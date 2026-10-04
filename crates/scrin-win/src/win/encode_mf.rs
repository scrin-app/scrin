//! Media Foundation H.264 encoding: hardware MFTs (async, event driven) with the Microsoft
//! software MFT (sync) as a fallback.
//!
//! Input is NV12 in system memory, converted from captured BGRA by [`crate::color::Nv12`].
//! Hardware MFTs are asynchronous: they announce `METransformNeedInput` / `METransformHaveOutput`
//! through `IMFMediaEventGenerator`; we poll the queue without blocking (bounded by a short
//! deadline) so a misbehaving driver can never hang the capture loop.
//!
//! Low-latency configuration through `ICodecAPI`: `AVLowLatencyMode`, CBR rate control, no
//! B-frames, a very long GOP (keyframes on request via `AVEncVideoForceKeyFrame`). Every codec
//! property is best effort — drivers differ in what they accept — and a rejected property is
//! logged, not fatal.

use std::mem::ManuallyDrop;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::E_NOTIMPL;
use windows::Win32::Media::MediaFoundation::{
    CODECAPI_AVEncCommonMeanBitRate, CODECAPI_AVEncCommonRateControlMode,
    CODECAPI_AVEncMPVDefaultBPictureCount, CODECAPI_AVEncMPVGOPSize,
    CODECAPI_AVEncVideoForceKeyFrame, CODECAPI_AVLowLatencyMode, ICodecAPI, IMFActivate,
    IMFAttributes, IMFMediaEventGenerator, IMFMediaType, IMFSample, IMFTransform,
    METransformHaveOutput, METransformNeedInput, MF_E_NO_EVENTS_AVAILABLE,
    MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE, MF_EVENT_FLAG_NO_WAIT,
    MF_LOW_LATENCY, MF_MT_ALL_SAMPLES_INDEPENDENT, MF_MT_AVG_BITRATE, MF_MT_DEFAULT_STRIDE,
    MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
    MF_MT_MPEG2_PROFILE, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE, MF_TRANSFORM_ASYNC,
    MF_TRANSFORM_ASYNC_UNLOCK, MF_VERSION, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
    MFMediaType_Video, MFSTARTUP_LITE, MFSampleExtension_CleanPoint, MFShutdown, MFStartup,
    MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG, MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER,
    MFT_ENUM_FLAG_SYNCMFT, MFT_ENUM_HARDWARE_VENDOR_ID_Attribute, MFT_FRIENDLY_NAME_Attribute,
    MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_END_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
    MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
    MFT_REGISTER_TYPE_INFO, MFTEnumEx, MFVideoFormat_H264, MFVideoFormat_NV12,
    MFVideoInterlace_Progressive, eAVEncCommonRateControlMode_CBR, eAVEncH264VProfile_Main,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Variant::VARIANT;
use windows::core::{GUID, Interface, PWSTR};

use super::{ComGuard, EncoderSettings, OsContext};
use crate::color::{BgraImage, Nv12};
use crate::{CapturedFrame, EncodedFrame, EncoderInfo, Error, PixelData, Result, VideoEncoder};

/// How long `encode` waits for the MFT to accept input or produce output.
const EVENT_DEADLINE: Duration = Duration::from_millis(40);

/// Keeps COM and Media Foundation started for the lifetime of an encoder.
#[derive(Debug)]
struct MfRuntime {
    _com: ComGuard,
}

impl MfRuntime {
    fn start() -> Result<Self> {
        let com = ComGuard::mta()?;
        // SAFETY: MFStartup is reference counted; balanced by MFShutdown in Drop.
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_LITE) }.ctx("MFStartup")?;
        Ok(Self { _com: com })
    }
}

impl Drop for MfRuntime {
    fn drop(&mut self) {
        // SAFETY: paired with the successful MFStartup in `start`.
        let _ = unsafe { MFShutdown() };
    }
}

fn read_string(attrs: &IMFAttributes, key: &GUID) -> Option<String> {
    let mut p = PWSTR::null();
    let mut len = 0u32;
    // SAFETY: out-parameters are live locals; on success `p` is CoTaskMemAlloc'ed and freed below.
    unsafe { attrs.GetAllocatedString(key, &raw mut p, &raw mut len) }.ok()?;
    // SAFETY: `p` is a valid NUL-terminated wide string returned by MF.
    let s = unsafe { p.to_string() }.ok();
    // SAFETY: allocated by GetAllocatedString with CoTaskMemAlloc.
    unsafe { CoTaskMemFree(Some(p.0.cast_const().cast())) };
    s
}

/// Enumerates H.264 encoder activation objects (NV12 in, H.264 out).
fn enum_activates(flags: MFT_ENUM_FLAG) -> Result<Vec<IMFActivate>> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };
    let mut array: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    // SAFETY: type infos outlive the call; out-parameters are live locals. The returned array is
    // CoTaskMemAlloc'ed and owns one reference per element.
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            flags,
            Some(&raw const input),
            Some(&raw const output),
            &raw mut array,
            &raw mut count,
        )
    }
    .ctx("MFTEnumEx")?;
    let mut out = Vec::with_capacity(count as usize);
    if !array.is_null() {
        for i in 0..count as usize {
            // SAFETY: `i < count`; each element is read (moved) exactly once, transferring its
            // reference to `out`; the array itself is then freed without touching elements.
            if let Some(a) = unsafe { array.add(i).read() } {
                out.push(a);
            }
        }
        // SAFETY: the array was allocated by MFTEnumEx with CoTaskMemAlloc.
        unsafe { CoTaskMemFree(Some(array.cast_const().cast())) };
    }
    Ok(out)
}

fn info_of(a: &IMFActivate, hardware: bool) -> EncoderInfo {
    let attrs: Option<IMFAttributes> = a.cast().ok();
    let get = |k: &GUID| {
        attrs
            .as_ref()
            .and_then(|x| read_string(x, k))
            .unwrap_or_default()
    };
    EncoderInfo {
        name: get(&MFT_FRIENDLY_NAME_Attribute),
        vendor: get(&MFT_ENUM_HARDWARE_VENDOR_ID_Attribute),
        hardware,
    }
}

/// Lists H.264 encoder MFTs: hardware (`true`) or synchronous software (`false`).
pub fn probe_encoders(hardware: bool) -> Result<Vec<EncoderInfo>> {
    let _rt = MfRuntime::start()?;
    let flags = if hardware {
        MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_SORTANDFILTER.0)
    } else {
        MFT_ENUM_FLAG(MFT_ENUM_FLAG_SYNCMFT.0 | MFT_ENUM_FLAG_SORTANDFILTER.0)
    };
    Ok(enum_activates(flags)?
        .iter()
        .map(|a| info_of(a, hardware))
        .collect())
}

fn pack(hi: u32, lo: u32) -> u64 {
    (u64::from(hi) << 32) | u64::from(lo)
}

fn video_type(subtype: &GUID, s: &EncoderSettings) -> Result<IMFMediaType> {
    // SAFETY: plain factory call.
    let t = unsafe { MFCreateMediaType() }.ctx("MFCreateMediaType")?;
    // SAFETY: `t` is a valid media type; GUID pointers reference constants.
    unsafe {
        t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .ctx("SetGUID(major)")?;
        t.SetGUID(&MF_MT_SUBTYPE, subtype).ctx("SetGUID(subtype)")?;
        t.SetUINT64(&MF_MT_FRAME_SIZE, pack(s.width, s.height))
            .ctx("FRAME_SIZE")?;
        t.SetUINT64(&MF_MT_FRAME_RATE, pack(s.fps.max(1), 1))
            .ctx("FRAME_RATE")?;
        t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))
            .ctx("PIXEL_ASPECT_RATIO")?;
        t.SetUINT32(
            &MF_MT_INTERLACE_MODE,
            MFVideoInterlace_Progressive.0.cast_unsigned(),
        )
        .ctx("INTERLACE")?;
        t.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)
            .ctx("ALL_SAMPLES_INDEPENDENT")?;
    }
    Ok(t)
}

/// One H.264 encoder MFT.
#[derive(Debug)]
pub struct MfEncoder {
    name: String,
    transform: IMFTransform,
    events: Option<IMFMediaEventGenerator>,
    codec: Option<ICodecAPI>,
    activate: IMFActivate,
    in_id: u32,
    out_id: u32,
    provides_samples: bool,
    out_size: u32,
    settings: EncoderSettings,
    need_input: u32,
    nv12: Nv12,
    ready: std::collections::VecDeque<EncodedFrame>,
    frames_in: u64,
    frames_out: u64,
    _rt: MfRuntime,
}

impl MfEncoder {
    /// Opens the first hardware H.264 MFT that accepts the configuration.
    pub fn new_hardware(settings: EncoderSettings) -> Result<Self> {
        Self::open_first(
            settings,
            MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_SORTANDFILTER.0),
            true,
        )
    }

    /// Opens the Microsoft software H.264 MFT (synchronous).
    pub fn new_software(settings: EncoderSettings) -> Result<Self> {
        Self::open_first(
            settings,
            MFT_ENUM_FLAG(MFT_ENUM_FLAG_SYNCMFT.0 | MFT_ENUM_FLAG_SORTANDFILTER.0),
            false,
        )
    }

    fn open_first(settings: EncoderSettings, flags: MFT_ENUM_FLAG, hardware: bool) -> Result<Self> {
        let settings = EncoderSettings {
            width: settings.width & !1,
            height: settings.height & !1,
            ..settings
        };
        if settings.width == 0 || settings.height == 0 {
            return Err(Error::InvalidInput(
                "encoder size must be at least 2x2".into(),
            ));
        }
        let _rt = MfRuntime::start()?;
        let mut last = Error::Unsupported(if hardware {
            "no hardware H.264 MFT"
        } else {
            "no software H.264 MFT"
        });
        for activate in enum_activates(flags)? {
            let info = info_of(&activate, hardware);
            match Self::open(activate, info.name.clone(), settings) {
                Ok(enc) => {
                    tracing::info!(name = %info.name, vendor = %info.vendor, async_mft = enc.events.is_some(), "H.264 MFT opened");
                    return Ok(enc);
                }
                Err(e) => {
                    tracing::info!(name = %info.name, error = %e, "H.264 MFT rejected configuration");
                    last = e;
                }
            }
        }
        Err(last)
    }

    fn open(activate: IMFActivate, name: String, settings: EncoderSettings) -> Result<Self> {
        let rt = MfRuntime::start()?;
        // SAFETY: valid activation object; creates the MFT.
        let transform: IMFTransform = unsafe { activate.ActivateObject() }.ctx("ActivateObject")?;
        let shutdown_on_err = |e: Error| {
            // SAFETY: valid activation object; releases the MFT it created.
            let _ = unsafe { activate.ShutdownObject() };
            e
        };
        // SAFETY: valid transform.
        let attrs = unsafe { transform.GetAttributes() }.ok();
        let mut is_async = false;
        if let Some(a) = &attrs {
            // SAFETY: valid attribute store; GUIDs are constants.
            is_async = unsafe { a.GetUINT32(&MF_TRANSFORM_ASYNC) }.unwrap_or(0) != 0;
            if is_async {
                // SAFETY: as above. Required before an async MFT accepts any call.
                unsafe { a.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1) }
                    .ctx("MF_TRANSFORM_ASYNC_UNLOCK")
                    .map_err(shutdown_on_err)?;
            }
            // SAFETY: as above; best effort.
            let _ = unsafe { a.SetUINT32(&MF_LOW_LATENCY, 1) };
        }

        let mut ins = [0u32];
        let mut outs = [0u32];
        // SAFETY: slices of length 1 for the single input/output stream.
        let (in_id, out_id) = match unsafe { transform.GetStreamIDs(&mut ins, &mut outs) } {
            Ok(()) => (ins[0], outs[0]),
            Err(e) if e.code() == E_NOTIMPL => (0, 0),
            Err(e) => return Err(shutdown_on_err(super::os_err("GetStreamIDs", &e))),
        };

        let codec: Option<ICodecAPI> = transform.cast().ok();
        let mut enc = Self {
            name,
            transform,
            events: None,
            codec,
            activate,
            in_id,
            out_id,
            provides_samples: false,
            out_size: 0,
            settings,
            need_input: 0,
            nv12: Nv12::default(),
            ready: std::collections::VecDeque::new(),
            frames_in: 0,
            frames_out: 0,
            _rt: rt,
        };
        enc.configure_codec_api();
        enc.set_types()?;
        enc.refresh_output_info()?;
        if is_async {
            enc.events = Some(enc.transform.cast().ctx("IMFMediaEventGenerator")?);
        }
        // SAFETY: valid transform; messages without parameters. FLUSH before streaming is a
        // no-op that some MFTs (the Microsoft software encoder) reject with E_FAIL, so it is
        // best effort.
        unsafe {
            let _ = enc.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
            enc.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .ctx("BEGIN_STREAMING")?;
            enc.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .ctx("START_OF_STREAM")?;
        }
        Ok(enc)
    }

    fn set_codec_value(&self, key: &GUID, value: &VARIANT, what: &str) {
        if let Some(c) = &self.codec {
            // SAFETY: valid ICodecAPI; `value` outlives the call.
            if let Err(e) = unsafe { c.SetValue(key, value) } {
                tracing::debug!(property = what, error = %e, "ICodecAPI property rejected");
            }
        }
    }

    /// Re-reads the output stream info (sample ownership and size), which can change with the
    /// output type.
    fn refresh_output_info(&mut self) -> Result<()> {
        // SAFETY: valid transform; stream id from GetStreamIDs.
        let info = unsafe { self.transform.GetOutputStreamInfo(self.out_id) }
            .ctx("GetOutputStreamInfo")?;
        self.provides_samples = info.dwFlags
            & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0)
                .cast_unsigned()
            != 0;
        self.out_size = info.cbSize.max(self.settings.width * self.settings.height);
        tracing::debug!(
            encoder = %self.name,
            flags = info.dwFlags,
            size = info.cbSize,
            provides_samples = self.provides_samples,
            "MFT output stream info"
        );
        Ok(())
    }

    fn configure_codec_api(&self) {
        self.set_codec_value(
            &CODECAPI_AVLowLatencyMode,
            &VARIANT::from(true),
            "AVLowLatencyMode",
        );
        self.set_codec_value(
            &CODECAPI_AVEncCommonRateControlMode,
            &VARIANT::from(eAVEncCommonRateControlMode_CBR.0.cast_unsigned()),
            "AVEncCommonRateControlMode",
        );
        self.set_codec_value(
            &CODECAPI_AVEncCommonMeanBitRate,
            &VARIANT::from(self.settings.bitrate),
            "MeanBitRate",
        );
        self.set_codec_value(
            &CODECAPI_AVEncMPVDefaultBPictureCount,
            &VARIANT::from(0u32),
            "BPictureCount",
        );
        self.set_codec_value(
            &CODECAPI_AVEncMPVGOPSize,
            &VARIANT::from(self.settings.fps.max(1) * 600),
            "GOPSize",
        );
    }

    /// Applies our size, rate, bitrate and profile to an H.264 output type.
    fn adjust_output_type(t: &IMFMediaType, s: &EncoderSettings) -> Result<()> {
        // SAFETY: valid media type; GUIDs are constants.
        unsafe {
            t.SetUINT64(&MF_MT_FRAME_SIZE, pack(s.width, s.height))
                .ctx("FRAME_SIZE")?;
            t.SetUINT64(&MF_MT_FRAME_RATE, pack(s.fps.max(1), 1))
                .ctx("FRAME_RATE")?;
            t.SetUINT32(
                &MF_MT_INTERLACE_MODE,
                MFVideoInterlace_Progressive.0.cast_unsigned(),
            )
            .ctx("INTERLACE")?;
            t.SetUINT32(&MF_MT_AVG_BITRATE, s.bitrate)
                .ctx("AVG_BITRATE")?;
            t.SetUINT32(
                &MF_MT_MPEG2_PROFILE,
                eAVEncH264VProfile_Main.0.cast_unsigned(),
            )
            .ctx("PROFILE")?;
        }
        Ok(())
    }

    fn set_types(&mut self) -> Result<()> {
        let s = self.settings;
        let out = video_type(&MFVideoFormat_H264, &s)?;
        Self::adjust_output_type(&out, &s)?;
        // SAFETY: valid transform and media type. Encoders need the output type first.
        unsafe { self.transform.SetOutputType(self.out_id, &out, 0) }.ctx("SetOutputType(H264)")?;
        let input = video_type(&MFVideoFormat_NV12, &s)?;
        // SAFETY: valid media type.
        unsafe { input.SetUINT32(&MF_MT_DEFAULT_STRIDE, s.width) }.ctx("DEFAULT_STRIDE")?;
        // SAFETY: valid transform and media type.
        unsafe { self.transform.SetInputType(self.in_id, &input, 0) }.ctx("SetInputType(NV12)")?;
        Ok(())
    }

    /// Whether this MFT is asynchronous (hardware).
    #[must_use]
    pub fn is_async(&self) -> bool {
        self.events.is_some()
    }

    fn make_input(&self, pts_us: u64) -> Result<IMFSample> {
        let len = u32::try_from(self.nv12.data.len())
            .map_err(|_| Error::InvalidInput("frame too large".into()))?;
        // SAFETY: plain allocation of an MF memory buffer of `len` bytes.
        let buffer = unsafe { MFCreateMemoryBuffer(len) }.ctx("MFCreateMemoryBuffer")?;
        let mut ptr: *mut u8 = std::ptr::null_mut();
        // SAFETY: Lock yields a writable pointer to at least `len` bytes until Unlock.
        unsafe { buffer.Lock(&raw mut ptr, None, None) }.ctx("IMFMediaBuffer::Lock")?;
        // SAFETY: `ptr` is valid for `len` bytes (max length requested above); source is `len` bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(self.nv12.data.as_ptr(), ptr, self.nv12.data.len());
        }
        // SAFETY: matches the Lock above; then set the valid length.
        unsafe {
            buffer.Unlock().ctx("IMFMediaBuffer::Unlock")?;
            buffer.SetCurrentLength(len).ctx("SetCurrentLength")?;
        }
        // SAFETY: plain factory call; then attach the buffer and timing (100 ns units).
        let sample = unsafe { MFCreateSample() }.ctx("MFCreateSample")?;
        let ts = i64::try_from(pts_us.saturating_mul(10)).unwrap_or(i64::MAX);
        let dur = 10_000_000 / i64::from(self.settings.fps.max(1));
        // SAFETY: valid sample and buffer.
        unsafe {
            sample.AddBuffer(&buffer).ctx("AddBuffer")?;
            sample.SetSampleTime(ts).ctx("SetSampleTime")?;
            sample.SetSampleDuration(dur).ctx("SetSampleDuration")?;
        }
        Ok(sample)
    }

    /// Handles `MF_E_TRANSFORM_STREAM_CHANGE`: picks the first available H.264 output type,
    /// re-applies our settings to it, sets it and re-reads the output stream info.
    fn renegotiate_output(&mut self) -> Result<()> {
        let mut chosen = None;
        for i in 0..32 {
            // SAFETY: valid transform; enumeration ends with MF_E_NO_MORE_TYPES.
            let Ok(t) = (unsafe { self.transform.GetOutputAvailableType(self.out_id, i) }) else {
                break;
            };
            // SAFETY: valid media type.
            let sub = unsafe { t.GetGUID(&MF_MT_SUBTYPE) }.unwrap_or_default();
            if sub == MFVideoFormat_H264 {
                chosen = Some(t);
                break;
            }
        }
        let t = match chosen {
            Some(t) => t,
            None => video_type(&MFVideoFormat_H264, &self.settings)?,
        };
        Self::adjust_output_type(&t, &self.settings)?;
        // SAFETY: valid transform and type.
        unsafe { self.transform.SetOutputType(self.out_id, &t, 0) }
            .ctx("SetOutputType(renegotiate)")?;
        tracing::debug!(encoder = %self.name, "MFT output type renegotiated after stream change");
        self.refresh_output_info()
    }

    /// Pulls one output sample. `Ok(false)` when there was nothing to collect (the MFT needs
    /// more input, or it changed its output format).
    ///
    /// Asynchronous MFTs allow exactly one `ProcessOutput` per `METransformHaveOutput` event —
    /// any other call fails with `E_UNEXPECTED` (0x8000FFFF). After a stream change the MFT
    /// sends a fresh `METransformHaveOutput`, so an async encoder must not retry here.
    fn pull_output(&mut self) -> Result<bool> {
        let attempts = if self.events.is_some() { 1 } else { 2 };
        for _ in 0..attempts {
            let own = if self.provides_samples {
                None
            } else {
                // SAFETY: plain allocations; the buffer is attached to the sample.
                let s = unsafe { MFCreateSample() }.ctx("MFCreateSample(out)")?;
                // SAFETY: as above.
                let b = unsafe { MFCreateMemoryBuffer(self.out_size) }
                    .ctx("MFCreateMemoryBuffer(out)")?;
                // SAFETY: valid sample/buffer.
                unsafe { s.AddBuffer(&b) }.ctx("AddBuffer(out)")?;
                Some(s)
            };
            let mut buffers = [MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: self.out_id,
                pSample: ManuallyDrop::new(own),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            }];
            let mut status = 0u32;
            // SAFETY: one output buffer for our single stream; ownership of pSample/pEvents is
            // taken back below whatever the result.
            let r = unsafe {
                self.transform
                    .ProcessOutput(0, &mut buffers, &raw mut status)
            };
            let [buf] = buffers;
            let sample = ManuallyDrop::into_inner(buf.pSample);
            drop(ManuallyDrop::into_inner(buf.pEvents));
            match r {
                Ok(()) => {
                    if let Some(s) = sample {
                        self.collect(&s)?;
                    }
                    return Ok(true);
                }
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(false),
                Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => self.renegotiate_output()?,
                Err(e) => return Err(super::os_err("ProcessOutput", &e)),
            }
        }
        Ok(false)
    }

    fn collect(&mut self, sample: &IMFSample) -> Result<()> {
        // SAFETY: valid sample from the MFT.
        let buffer =
            unsafe { sample.ConvertToContiguousBuffer() }.ctx("ConvertToContiguousBuffer")?;
        let mut ptr: *mut u8 = std::ptr::null_mut();
        let mut len = 0u32;
        // SAFETY: Lock yields a readable pointer to `len` valid bytes until Unlock.
        unsafe { buffer.Lock(&raw mut ptr, None, Some(&raw mut len)) }.ctx("Lock(out)")?;
        // SAFETY: as above.
        let data = unsafe { std::slice::from_raw_parts(ptr, len as usize) }.to_vec();
        // SAFETY: matches the Lock above.
        unsafe { buffer.Unlock() }.ctx("Unlock(out)")?;
        // SAFETY: valid sample; missing attributes are reported as errors and mapped to defaults.
        let clean = unsafe { sample.GetUINT32(&MFSampleExtension_CleanPoint) }.unwrap_or(0) != 0;
        // SAFETY: as above.
        let ts = unsafe { sample.GetSampleTime() }.unwrap_or(0);
        let keyframe = clean || has_idr(&data);
        self.frames_out += 1;
        self.ready.push_back(EncodedFrame {
            data,
            keyframe,
            pts_us: u64::try_from(ts / 10).unwrap_or(0),
        });
        Ok(())
    }

    /// Drains pending async events; returns once `want` is satisfied or the deadline passes.
    fn pump(&mut self, deadline: Instant, want: impl Fn(&Self) -> bool) -> Result<()> {
        let Some(events) = self.events.clone() else {
            return Ok(());
        };
        loop {
            // SAFETY: valid event generator; NO_WAIT never blocks.
            match unsafe { events.GetEvent(MF_EVENT_FLAG_NO_WAIT) } {
                Ok(ev) => {
                    // SAFETY: valid event.
                    let kind = unsafe { ev.GetType() }.ctx("IMFMediaEvent::GetType")?;
                    if kind == METransformNeedInput.0.cast_unsigned() {
                        self.need_input += 1;
                    } else if kind == METransformHaveOutput.0.cast_unsigned() {
                        self.pull_output()?;
                    }
                }
                Err(e) if e.code() == MF_E_NO_EVENTS_AVAILABLE => {
                    if want(self) || Instant::now() >= deadline {
                        return Ok(());
                    }
                    std::thread::sleep(Duration::from_micros(250));
                }
                Err(e) => return Err(super::os_err("IMFMediaEventGenerator::GetEvent", &e)),
            }
        }
    }

    /// Encodes one NV12 frame already in `self.nv12`.
    fn encode_nv12(&mut self, pts_us: u64, force_keyframe: bool) -> Result<Option<EncodedFrame>> {
        if force_keyframe {
            self.set_codec_value(
                &CODECAPI_AVEncVideoForceKeyFrame,
                &VARIANT::from(1u32),
                "ForceKeyFrame",
            );
        }
        let sample = self.make_input(pts_us)?;
        if self.events.is_some() {
            self.pump(Instant::now() + EVENT_DEADLINE, |s| s.need_input > 0)?;
            if self.need_input == 0 {
                tracing::debug!("hardware encoder did not request input in time; dropping frame");
                return Ok(self.ready.pop_front());
            }
            // SAFETY: valid transform and sample; the MFT requested input.
            unsafe { self.transform.ProcessInput(self.in_id, &sample, 0) }.ctx("ProcessInput")?;
            self.need_input -= 1;
            self.frames_in += 1;
            let target = self.frames_in;
            // Wait for this frame's output (low-latency MFTs emit it before asking for more).
            self.pump(Instant::now() + EVENT_DEADLINE, move |s| {
                s.frames_out >= target || s.need_input > 0
            })?;
        } else {
            // SAFETY: valid transform and sample.
            match unsafe { self.transform.ProcessInput(self.in_id, &sample, 0) } {
                Ok(()) => {}
                Err(e) => return Err(super::os_err("ProcessInput", &e)),
            }
            self.frames_in += 1;
            while self.pull_output()? {}
        }
        Ok(self.ready.pop_front())
    }
}

impl Drop for MfEncoder {
    fn drop(&mut self) {
        // SAFETY: valid transform/activation; best-effort teardown before COM/MF shut down
        // (field drop order releases the transform before `_rt`).
        unsafe {
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
            let _ = self.activate.ShutdownObject();
        }
    }
}

/// Whether an Annex B access unit contains an IDR slice (NAL type 5).
#[must_use]
pub fn has_idr(data: &[u8]) -> bool {
    data.windows(4)
        .any(|w| w[0] == 0 && w[1] == 0 && w[2] == 1 && (w[3] & 0x1F) == 5)
}

impl VideoEncoder for MfEncoder {
    fn name(&self) -> &str {
        &self.name
    }

    fn encode(
        &mut self,
        frame: &CapturedFrame<'_>,
        force_keyframe: bool,
    ) -> Result<Option<EncodedFrame>> {
        let PixelData::Cpu(bytes) = frame.data else {
            return Err(Error::InvalidInput(
                "MF encoder path takes CPU frames (open the capture with want_cpu)".into(),
            ));
        };
        let (w, h) = (self.settings.width as usize, self.settings.height as usize);
        if (frame.width as usize) < w || (frame.height as usize) < h {
            return Err(Error::InvalidInput(format!(
                "frame {}x{} smaller than encoder {w}x{h}",
                frame.width, frame.height
            )));
        }
        // Crops to the encoder's (even) size.
        self.nv12.fill_from_bgra(&BgraImage {
            data: bytes,
            width: w,
            height: h,
            stride: frame.stride,
        })?;
        self.encode_nv12(frame.pts_us, force_keyframe || frame.discontinuity)
    }

    fn set_bitrate(&mut self, bps: u32) -> Result<()> {
        self.settings.bitrate = bps;
        self.set_codec_value(
            &CODECAPI_AVEncCommonMeanBitRate,
            &VARIANT::from(bps),
            "MeanBitRate",
        );
        Ok(())
    }

    fn set_fps(&mut self, fps: u32) -> Result<()> {
        // MF frame rate is part of the negotiated media type; mid-stream we only adjust sample
        // durations, which is what CBR rate control uses.
        self.settings.fps = fps.max(1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VideoDecoder;
    use crate::win::decode_openh264::OpenH264Decoder;
    use crate::win::encode_openh264::tests::{frame, synthetic_bgra};

    #[test]
    fn idr_detection() {
        assert!(has_idr(&[0, 0, 0, 1, 0x67, 1, 0, 0, 1, 0x65, 9]));
        assert!(!has_idr(&[0, 0, 0, 1, 0x41, 1, 2]));
    }

    #[test]
    fn probe_hardware_encoders_does_not_crash() {
        let hw = crate::probe_hardware_encoders().expect("MFTEnumEx");
        for e in &hw {
            assert!(e.hardware);
            eprintln!("hardware H.264 MFT: {} ({})", e.name, e.vendor);
        }
        let sw = probe_encoders(false).expect("MFTEnumEx software");
        assert!(sw.iter().all(|e| !e.hardware));
        eprintln!(
            "software H.264 MFTs: {:?}",
            sw.iter().map(|e| &e.name).collect::<Vec<_>>()
        );
    }

    fn round_trip(mut enc: MfEncoder) {
        let (w, h) = (640u32, 360u32);
        let mut dec = OpenH264Decoder::new().expect("decoder");
        let mut outputs = Vec::new();
        for t in 0..30usize {
            let img = synthetic_bgra(w as usize, h as usize, t);
            if let Some(ef) = enc
                .encode(&frame(&img, w, h, t as u64 * 33_333), t == 0)
                .expect("encode")
            {
                outputs.push(ef);
            }
        }
        assert!(
            outputs.len() >= 20,
            "{} produced only {} outputs for 30 inputs",
            enc.name(),
            outputs.len()
        );
        assert!(outputs[0].keyframe, "first output must be a keyframe");
        let mut pictures = 0;
        for o in &outputs {
            if let Some(p) = dec.decode(&o.data).expect("decodable by openh264") {
                assert_eq!((p.width, p.height), (w as usize, h as usize));
                pictures += 1;
            }
        }
        assert!(
            pictures >= outputs.len() - 1,
            "{pictures} pictures from {} outputs",
            outputs.len()
        );
    }

    #[test]
    fn software_mft_round_trip() {
        let enc = MfEncoder::new_software(EncoderSettings {
            width: 640,
            height: 360,
            fps: 30,
            bitrate: 2_000_000,
        })
        .expect("Microsoft H.264 encoder MFT is part of Windows");
        assert!(!enc.is_async());
        round_trip(enc);
    }

    #[test]
    fn hardware_mft_round_trip_when_available() {
        match MfEncoder::new_hardware(EncoderSettings {
            width: 640,
            height: 360,
            fps: 30,
            bitrate: 2_000_000,
        }) {
            Ok(enc) => {
                eprintln!(
                    "hardware encoder: {} (async={})",
                    enc.name(),
                    enc.is_async()
                );
                round_trip(enc);
            }
            Err(e) => eprintln!("no hardware H.264 encoder usable here: {e}"),
        }
    }
}
