//! WASAPI loopback capture of the default render endpoint → 10 ms 48 kHz stereo i16 frames,
//! plus an optional Opus encoder for those frames.
//!
//! Shared mode, event driven: the audio engine signals an event every period; we drain all
//! packets, convert the mix format (float32 or PCM16, any channel count) to stereo f32,
//! resample to 48 kHz when needed and cut 10 ms frames. Silent packets (nothing playing) are
//! turned into zeros so timing stays continuous while something is playing; when the engine
//! delivers nothing at all (fully idle endpoint) `next_frame` simply times out.
//!
//! Process-excluding loopback (so scrin does not capture its own output) needs
//! `ActivateAudioInterfaceAsync` with `AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK`; that is a
//! follow-up and is not implemented here.

use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
    AUDCLNT_STREAMFLAGS_LOOPBACK, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator,
    MMDeviceEnumerator, WAVE_FORMAT_PCM, WAVEFORMATEX, WAVEFORMATEXTENSIBLE, eConsole, eRender,
};
use windows::Win32::Media::KernelStreaming::{KSDATAFORMAT_SUBTYPE_PCM, WAVE_FORMAT_EXTENSIBLE};
use windows::Win32::Media::Multimedia::{KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, WAVE_FORMAT_IEEE_FLOAT};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

use super::{ComGuard, OsContext};
use crate::resample::{Framer, LinearResampler, to_stereo};
use crate::{AUDIO_SAMPLE_RATE, AudioCapture, AudioFrame, Error, Result};

/// Sample encoding of the endpoint mix format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    /// 32-bit IEEE float.
    F32,
    /// 16-bit signed PCM.
    I16,
    /// 24-bit PCM in a 32-bit container, or 32-bit PCM.
    I32,
}

/// Decoded mix format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MixFormat {
    /// Samples per second.
    pub rate: u32,
    /// Interleaved channels.
    pub channels: usize,
    /// Sample encoding.
    pub format: SampleFormat,
}

impl MixFormat {
    fn bytes_per_sample(self) -> usize {
        match self.format {
            SampleFormat::I16 => 2,
            SampleFormat::F32 | SampleFormat::I32 => 4,
        }
    }
}

/// Interprets a `WAVEFORMATEX` (possibly `WAVEFORMATEXTENSIBLE`).
///
/// # Safety
/// `fmt` must point at a valid `WAVEFORMATEX`; when its tag is `WAVE_FORMAT_EXTENSIBLE` the
/// allocation must hold a full `WAVEFORMATEXTENSIBLE`.
unsafe fn parse_format(fmt: *const WAVEFORMATEX) -> Result<MixFormat> {
    // SAFETY: caller guarantees a valid WAVEFORMATEX; the struct is packed, so read unaligned.
    let base = unsafe { fmt.read_unaligned() };
    let tag = u32::from(base.wFormatTag);
    let bits = base.wBitsPerSample;
    let kind = if tag == WAVE_FORMAT_EXTENSIBLE {
        // SAFETY: caller guarantees the extensible layout for this tag.
        let ext = unsafe { fmt.cast::<WAVEFORMATEXTENSIBLE>().read_unaligned() };
        let sub = ext.SubFormat;
        if sub == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT {
            WAVE_FORMAT_IEEE_FLOAT
        } else if sub == KSDATAFORMAT_SUBTYPE_PCM {
            WAVE_FORMAT_PCM
        } else {
            0
        }
    } else {
        tag
    };
    let format = match (kind, bits) {
        (WAVE_FORMAT_IEEE_FLOAT, 32) => SampleFormat::F32,
        (WAVE_FORMAT_PCM, 16) => SampleFormat::I16,
        (WAVE_FORMAT_PCM, 24 | 32) if base.nBlockAlign == base.nChannels * 4 => SampleFormat::I32,
        _ => return Err(Error::Unsupported("unsupported WASAPI mix format")),
    };
    Ok(MixFormat {
        rate: base.nSamplesPerSec,
        channels: usize::from(base.nChannels.max(1)),
        format,
    })
}

/// Converts one packet of interleaved samples to interleaved stereo f32.
pub fn packet_to_stereo_f32(bytes: &[u8], fmt: MixFormat, out: &mut Vec<f32>) {
    let frame_bytes = fmt.channels * fmt.bytes_per_sample();
    let mut tmp = Vec::with_capacity(fmt.channels);
    let mut st = [0.0f32; 2];
    for frame in bytes.chunks_exact(frame_bytes) {
        tmp.clear();
        match fmt.format {
            SampleFormat::F32 => {
                tmp.extend(
                    frame
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| f32::from_le_bytes(*b)),
                );
            }
            SampleFormat::I16 => {
                tmp.extend(
                    frame
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| f32::from(i16::from_le_bytes(*b)) / 32_768.0),
                );
            }
            SampleFormat::I32 => {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "audio samples; 24 significant bits"
                )]
                tmp.extend(
                    frame
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| i32::from_le_bytes(*b) as f32 / 2_147_483_648.0),
                );
            }
        }
        to_stereo(&tmp, &mut st);
        out.extend_from_slice(&st);
    }
}

/// Loopback capture of the default output device.
#[derive(Debug)]
pub struct LoopbackCapture {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    event: HANDLE,
    format: MixFormat,
    resampler: LinearResampler,
    framer: Framer,
    stereo: Vec<f32>,
    resampled: Vec<f32>,
    frame: Vec<i16>,
    frames_out: u64,
    _com: ComGuard,
}

impl LoopbackCapture {
    /// Opens loopback capture of the default render endpoint and starts it.
    pub fn new() -> Result<Self> {
        let com = ComGuard::mta()?;
        // SAFETY: standard COM activation of the device enumerator.
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                .ctx("MMDeviceEnumerator")?;
        // SAFETY: valid enumerator.
        let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
            .ctx("GetDefaultAudioEndpoint")?;
        // SAFETY: valid device; no activation parameters.
        let client: IAudioClient =
            unsafe { device.Activate(CLSCTX_ALL, None) }.ctx("IMMDevice::Activate")?;
        // SAFETY: valid client; the returned pointer is CoTaskMemAlloc'ed and freed below.
        let mix = unsafe { client.GetMixFormat() }.ctx("GetMixFormat")?;
        // SAFETY: `mix` comes from GetMixFormat, which allocates the full (extensible) struct.
        let parsed = unsafe { parse_format(mix) };
        // 20 ms shared-mode buffer; periodicity must be 0 in shared mode.
        // SAFETY: valid client and the engine's own mix format pointer.
        let init = unsafe {
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                200_000,
                0,
                mix,
                None,
            )
        };
        // SAFETY: allocated by GetMixFormat; not used after this point.
        unsafe { CoTaskMemFree(Some(mix.cast_const().cast())) };
        let format = parsed?;
        init.ctx("IAudioClient::Initialize(loopback)")?;
        // SAFETY: auto-reset unnamed event; closed in Drop.
        let event = unsafe { CreateEventW(None, false, false, None) }.ctx("CreateEventW")?;
        let started = (|| {
            // SAFETY: valid client and event handle.
            unsafe { client.SetEventHandle(event) }.ctx("SetEventHandle")?;
            // SAFETY: valid, initialised client.
            let capture: IAudioCaptureClient =
                unsafe { client.GetService() }.ctx("GetService(IAudioCaptureClient)")?;
            // SAFETY: valid, initialised client.
            unsafe { client.Start() }.ctx("IAudioClient::Start")?;
            Ok(capture)
        })();
        let capture = match started {
            Ok(c) => c,
            Err(e) => {
                // SAFETY: created above and not shared.
                let _ = unsafe { CloseHandle(event) };
                return Err(e);
            }
        };
        tracing::info!(rate = format.rate, channels = format.channels, format = ?format.format, "WASAPI loopback started");
        Ok(Self {
            client,
            capture,
            event,
            format,
            resampler: LinearResampler::new(format.rate, AUDIO_SAMPLE_RATE),
            framer: Framer::default(),
            stereo: Vec::new(),
            resampled: Vec::new(),
            frame: vec![0; Framer::FRAME_LEN],
            frames_out: 0,
            _com: com,
        })
    }

    /// The endpoint mix format.
    #[must_use]
    pub fn mix_format(&self) -> MixFormat {
        self.format
    }

    fn drain(&mut self) -> Result<()> {
        loop {
            // SAFETY: valid capture client.
            let next = unsafe { self.capture.GetNextPacketSize() }.ctx("GetNextPacketSize")?;
            if next == 0 {
                return Ok(());
            }
            let mut data: *mut u8 = std::ptr::null_mut();
            let mut frames = 0u32;
            let mut flags = 0u32;
            // SAFETY: out-parameters are live locals; paired with ReleaseBuffer below.
            unsafe {
                self.capture
                    .GetBuffer(&raw mut data, &raw mut frames, &raw mut flags, None, None)
            }
            .ctx("IAudioCaptureClient::GetBuffer")?;
            self.stereo.clear();
            if flags & AUDCLNT_BUFFERFLAGS_SILENT.0.cast_unsigned() != 0 || data.is_null() {
                self.stereo.resize(frames as usize * 2, 0.0);
            } else {
                let len = frames as usize * self.format.channels * self.format.bytes_per_sample();
                // SAFETY: the engine guarantees `frames` complete frames at `data` until ReleaseBuffer.
                let bytes = unsafe { std::slice::from_raw_parts(data, len) };
                packet_to_stereo_f32(bytes, self.format, &mut self.stereo);
            }
            // SAFETY: releases exactly the packet obtained above.
            unsafe { self.capture.ReleaseBuffer(frames) }
                .ctx("IAudioCaptureClient::ReleaseBuffer")?;
            self.resampled.clear();
            self.resampler.process(&self.stereo, &mut self.resampled);
            self.framer.push_f32(&self.resampled);
        }
    }
}

impl AudioCapture for LoopbackCapture {
    fn next_frame(&mut self, timeout: Duration) -> Result<Option<AudioFrame<'_>>> {
        if self.framer.pending_frames() < crate::AUDIO_FRAME_SAMPLES {
            let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
            // SAFETY: valid event handle owned by self.
            let r = unsafe { WaitForSingleObject(self.event, ms) };
            if r == WAIT_OBJECT_0 {
                self.drain()?;
            }
        }
        let Some(samples) = self.framer.pop() else {
            return Ok(None);
        };
        self.frame.copy_from_slice(samples);
        let pts_us = self.frames_out * 10_000;
        self.frames_out += 1;
        Ok(Some(AudioFrame {
            samples: &self.frame,
            pts_us,
        }))
    }
}

impl Drop for LoopbackCapture {
    fn drop(&mut self) {
        // SAFETY: valid client/event; best-effort teardown.
        unsafe {
            let _ = self.client.Stop();
            let _ = CloseHandle(self.event);
        }
    }
}

/// Opus encoder for 10 ms stereo 48 kHz frames (low-delay application, CBR-ish VBR off).
#[cfg(feature = "opus")]
pub struct OpusEncoder {
    enc: opus::Encoder,
    out: Vec<u8>,
}

#[cfg(feature = "opus")]
impl std::fmt::Debug for OpusEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpusEncoder").finish_non_exhaustive()
    }
}

#[cfg(feature = "opus")]
impl OpusEncoder {
    /// Creates an encoder at `bitrate` bits per second.
    pub fn new(bitrate: i32) -> Result<Self> {
        let map = |e: opus::Error| Error::Codec(format!("opus: {e}"));
        let mut enc = opus::Encoder::new(
            AUDIO_SAMPLE_RATE,
            opus::Channels::Stereo,
            opus::Application::LowDelay,
        )
        .map_err(map)?;
        enc.set_bitrate(opus::Bitrate::Bits(bitrate)).map_err(map)?;
        enc.set_vbr(false).map_err(map)?;
        Ok(Self {
            enc,
            out: vec![0; 1500],
        })
    }

    /// Encodes one 10 ms frame (960 interleaved samples) into a packet.
    pub fn encode(&mut self, frame: &[i16]) -> Result<&[u8]> {
        if frame.len() != Framer::FRAME_LEN {
            return Err(Error::InvalidInput(format!(
                "opus frame must be {} samples",
                Framer::FRAME_LEN
            )));
        }
        let n = self
            .enc
            .encode(frame, &mut self.out)
            .map_err(|e| Error::Codec(format!("opus: {e}")))?;
        Ok(&self.out[..n])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_conversion_formats() {
        let mut out = Vec::new();
        let f32_fmt = MixFormat {
            rate: 48_000,
            channels: 2,
            format: SampleFormat::F32,
        };
        let bytes: Vec<u8> = [0.5f32, -0.25]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        packet_to_stereo_f32(&bytes, f32_fmt, &mut out);
        assert_eq!(out, vec![0.5, -0.25]);

        out.clear();
        let i16_mono = MixFormat {
            rate: 44_100,
            channels: 1,
            format: SampleFormat::I16,
        };
        let bytes: Vec<u8> = [16_384i16, -32_768]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        packet_to_stereo_f32(&bytes, i16_mono, &mut out);
        assert_eq!(out, vec![0.5, 0.5, -1.0, -1.0]);

        out.clear();
        let surround = MixFormat {
            rate: 48_000,
            channels: 6,
            format: SampleFormat::I32,
        };
        let bytes: Vec<u8> = [1i32 << 30, -(1 << 30), 7, 7, 7, 7]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        packet_to_stereo_f32(&bytes, surround, &mut out);
        assert_eq!(out, vec![0.5, -0.5]);
    }

    #[test]
    fn parses_extensible_float() {
        let mut ext = WAVEFORMATEXTENSIBLE::default();
        ext.Format.wFormatTag = u16::try_from(WAVE_FORMAT_EXTENSIBLE).expect("tag");
        ext.Format.nChannels = 2;
        ext.Format.nSamplesPerSec = 44_100;
        ext.Format.wBitsPerSample = 32;
        ext.Format.nBlockAlign = 8;
        ext.SubFormat = KSDATAFORMAT_SUBTYPE_IEEE_FLOAT;
        // SAFETY: `ext` is a full, initialised WAVEFORMATEXTENSIBLE.
        let f = unsafe { parse_format((&raw const ext).cast()) }.expect("format");
        assert_eq!(
            f,
            MixFormat {
                rate: 44_100,
                channels: 2,
                format: SampleFormat::F32
            }
        );
    }

    #[test]
    fn loopback_starts_and_times_out_cleanly() {
        let mut cap = match LoopbackCapture::new() {
            Ok(c) => c,
            Err(e) => return eprintln!("skipped (no render endpoint?): {e}"),
        };
        eprintln!("mix format: {:?}", cap.mix_format());
        // Whether anything is playing is unknown; the call must return within the timeout.
        let t = std::time::Instant::now();
        let r = cap
            .next_frame(Duration::from_millis(30))
            .expect("next_frame");
        if let Some(f) = r {
            assert_eq!(f.samples.len(), 960);
        }
        assert!(t.elapsed() < Duration::from_millis(500));
    }

    #[cfg(feature = "opus")]
    #[test]
    fn opus_encodes_10ms_frames() {
        let mut enc = OpusEncoder::new(64_000).expect("opus");
        let frame: Vec<i16> = (0..960)
            .map(|i| {
                #[expect(clippy::cast_precision_loss, reason = "test signal")]
                let t = (i / 2) as f32 / 48_000.0;
                #[expect(clippy::cast_possible_truncation, reason = "bounded by 8000")]
                let s = ((t * 440.0 * std::f32::consts::TAU).sin() * 8_000.0) as i16;
                s
            })
            .collect();
        let packet = enc.encode(&frame).expect("encode").to_vec();
        assert!(!packet.is_empty() && packet.len() < 400);
        let mut dec = opus::Decoder::new(48_000, opus::Channels::Stereo).expect("decoder");
        let mut pcm = vec![0i16; 960];
        assert_eq!(dec.decode(&packet, &mut pcm, false).expect("decode"), 480);
        assert!(enc.encode(&frame[..10]).is_err());
    }
}
