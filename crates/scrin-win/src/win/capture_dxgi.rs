//! DXGI Desktop Duplication capture of one output.
//!
//! Each acquired frame is copied on the GPU into a texture we own and the duplication frame is
//! released immediately, so the desktop compositor is never held up by the encoder. When CPU
//! pixels are requested the copy goes through a reusable staging texture into a reusable buffer.
//!
//! `DXGI_ERROR_ACCESS_LOST` (desktop switch, UAC prompt, mode change, fullscreen app) drops the
//! duplication; the next call recreates it and flags the frame as a discontinuity so encoders emit
//! a keyframe. While the secure desktop is up, recreation fails with `E_ACCESSDENIED` and the
//! source reports "no frame" until it can attach again (capturing the secure desktop needs the
//! SYSTEM-launched agent, ADR-0005).

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{E_ACCESSDENIED, HMODULE, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_FLAG,
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING, D3D11CreateDevice,
    ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_NOT_FOUND, DXGI_ERROR_WAIT_TIMEOUT,
    DXGI_OUTDUPL_FRAME_INFO, DXGI_OUTDUPL_MOVE_RECT, DXGI_OUTDUPL_POINTER_SHAPE_INFO,
    IDXGIAdapter1, IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
};
use windows::Win32::Graphics::Gdi::{DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW};
use windows::core::{Interface, PCWSTR};

use super::OsContext;
use crate::cursor::{ShapeType, shape_to_rgba};
use crate::{
    CaptureSource, CapturedFrame, CursorInfo, DisplayInfo, Error, MoveRect, PixelData, Rect, Result,
};

fn wide_to_string(w: &[u16]) -> String {
    let end = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..end])
}

fn rect(r: RECT) -> Rect {
    Rect {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

struct Found {
    adapter: IDXGIAdapter1,
    output: IDXGIOutput1,
    info: DisplayInfo,
}

/// Enumerates attached outputs in adapter/output order.
fn enumerate() -> Result<Vec<Found>> {
    // SAFETY: plain factory creation; the returned interface is reference counted.
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ctx("CreateDXGIFactory1")?;
    let mut found = Vec::new();
    let mut index = 0;
    for a in 0.. {
        // SAFETY: `factory` is a valid interface; out-of-range indices return NOT_FOUND.
        let adapter = match unsafe { factory.EnumAdapters1(a) } {
            Ok(x) => x,
            Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(e) => return Err(super::os_err("EnumAdapters1", &e)),
        };
        // SAFETY: valid adapter interface.
        let adesc = unsafe { adapter.GetDesc1() }.ctx("IDXGIAdapter1::GetDesc1")?;
        let adapter_name = wide_to_string(&adesc.Description);
        for o in 0.. {
            // SAFETY: valid adapter; out-of-range indices return NOT_FOUND.
            let output = match unsafe { adapter.EnumOutputs(o) } {
                Ok(x) => x,
                Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(e) => return Err(super::os_err("EnumOutputs", &e)),
            };
            // SAFETY: valid output interface.
            let desc = unsafe { output.GetDesc() }.ctx("IDXGIOutput::GetDesc")?;
            if !desc.AttachedToDesktop.as_bool() {
                continue;
            }
            let output: IDXGIOutput1 = output.cast().ctx("IDXGIOutput1")?;
            let bounds = rect(desc.DesktopCoordinates);
            let mut mode = DEVMODEW {
                dmSize: u16::try_from(size_of::<DEVMODEW>()).unwrap_or(u16::MAX),
                ..DEVMODEW::default()
            };
            // SAFETY: DeviceName is NUL-terminated by DXGI; `mode` is a live, sized DEVMODEW.
            let ok = unsafe {
                EnumDisplaySettingsW(
                    PCWSTR(desc.DeviceName.as_ptr()),
                    ENUM_CURRENT_SETTINGS,
                    &raw mut mode,
                )
            };
            found.push(Found {
                adapter: adapter.clone(),
                output,
                info: DisplayInfo {
                    index,
                    name: wide_to_string(&desc.DeviceName),
                    adapter: adapter_name.clone(),
                    bounds,
                    primary: bounds.left == 0 && bounds.top == 0,
                    refresh_hz: if ok.as_bool() {
                        mode.dmDisplayFrequency
                    } else {
                        0
                    },
                },
            });
            index += 1;
        }
    }
    Ok(found)
}

/// Lists attached displays.
pub fn list_displays() -> Result<Vec<DisplayInfo>> {
    Ok(enumerate()?.into_iter().map(|f| f.info).collect())
}

/// Bounds of the whole virtual desktop (union of all displays).
pub fn virtual_desktop() -> Result<Rect> {
    let displays = list_displays()?;
    let mut it = displays.iter().map(|d| d.bounds);
    let first = it.next().ok_or(Error::Unsupported("no display attached"))?;
    Ok(it.fold(first, |a, b| Rect {
        left: a.left.min(b.left),
        top: a.top.min(b.top),
        right: a.right.max(b.right),
        bottom: a.bottom.max(b.bottom),
    }))
}

fn create_device(adapter: &IDXGIAdapter1) -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    let try_flags = |flags: D3D11_CREATE_DEVICE_FLAG| {
        let mut device = None;
        let mut context = None;
        // SAFETY: out-pointers reference live locals; a non-null adapter requires DRIVER_TYPE_UNKNOWN.
        unsafe {
            D3D11CreateDevice(
                adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                flags,
                None,
                D3D11_SDK_VERSION,
                Some(&raw mut device),
                None,
                Some(&raw mut context),
            )
        }
        .map(|()| (device, context))
    };
    let (device, context) =
        try_flags(D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT)
            .or_else(|_| try_flags(D3D11_CREATE_DEVICE_BGRA_SUPPORT))
            .ctx("D3D11CreateDevice")?;
    match (device, context) {
        (Some(d), Some(c)) => Ok((d, c)),
        _ => Err(Error::Unsupported("D3D11CreateDevice returned no device")),
    }
}

/// Desktop Duplication capture of one display.
#[derive(Debug)]
pub struct DxgiCapture {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    output: IDXGIOutput1,
    dupl: Option<IDXGIOutputDuplication>,
    display: DisplayInfo,
    want_cpu: bool,
    width: u32,
    height: u32,
    frame: Option<ID3D11Texture2D>,
    staging: Option<ID3D11Texture2D>,
    cpu: Vec<u8>,
    have_image: bool,
    discontinuity: bool,
    raw_dirty: Vec<RECT>,
    raw_moves: Vec<DXGI_OUTDUPL_MOVE_RECT>,
    dirty: Vec<Rect>,
    moves: Vec<MoveRect>,
    shape_buf: Vec<u8>,
    start: Instant,
}

impl DxgiCapture {
    /// Opens display `index` (see [`list_displays`]). With `want_cpu`, frames carry BGRA bytes;
    /// otherwise they carry the GPU texture.
    pub fn new(index: u32, want_cpu: bool) -> Result<Self> {
        let found = enumerate()?
            .into_iter()
            .find(|f| f.info.index == index)
            .ok_or_else(|| Error::InvalidInput(format!("no display {index}")))?;
        Self::open(found, want_cpu)
    }

    /// Opens the primary display.
    pub fn primary(want_cpu: bool) -> Result<Self> {
        let all = enumerate()?;
        let found = all
            .into_iter()
            .min_by_key(|f| !f.info.primary)
            .ok_or(Error::Unsupported("no display attached"))?;
        Self::open(found, want_cpu)
    }

    fn open(found: Found, want_cpu: bool) -> Result<Self> {
        let (device, context) = create_device(&found.adapter)?;
        let mut s = Self {
            device,
            context,
            output: found.output,
            dupl: None,
            width: found.info.bounds.width(),
            height: found.info.bounds.height(),
            display: found.info,
            want_cpu,
            frame: None,
            staging: None,
            cpu: Vec::new(),
            have_image: false,
            discontinuity: false,
            raw_dirty: Vec::new(),
            raw_moves: Vec::new(),
            dirty: Vec::new(),
            moves: Vec::new(),
            shape_buf: Vec::new(),
            start: Instant::now(),
        };
        s.duplicate()?;
        s.discontinuity = false;
        Ok(s)
    }

    /// The display being captured.
    #[must_use]
    pub fn display(&self) -> &DisplayInfo {
        &self.display
    }

    /// The D3D11 device frames live on (for GPU encoders).
    #[must_use]
    pub fn device(&self) -> &ID3D11Device {
        &self.device
    }

    fn duplicate(&mut self) -> Result<()> {
        // SAFETY: valid output and device interfaces.
        let dupl = unsafe { self.output.DuplicateOutput(&self.device) }.ctx("DuplicateOutput")?;
        // SAFETY: valid duplication interface.
        let desc = unsafe { dupl.GetDesc() };
        if (desc.ModeDesc.Width, desc.ModeDesc.Height) != (self.width, self.height)
            || self.frame.is_none()
        {
            self.width = desc.ModeDesc.Width;
            self.height = desc.ModeDesc.Height;
            self.frame = None;
            self.staging = None;
            self.have_image = false;
        }
        // SAFETY: valid output interface.
        if let Ok(od) = unsafe { self.output.GetDesc() } {
            self.display.bounds = rect(od.DesktopCoordinates);
        }
        self.dupl = Some(dupl);
        self.discontinuity = true;
        Ok(())
    }

    fn ensure_textures(&mut self, like: &ID3D11Texture2D) -> Result<()> {
        if self.frame.is_some() && (self.staging.is_some() || !self.want_cpu) {
            return Ok(());
        }
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: `desc` is a live out-parameter.
        unsafe { like.GetDesc(&raw mut desc) };
        self.width = desc.Width;
        self.height = desc.Height;
        let own = D3D11_TEXTURE2D_DESC {
            MipLevels: 1,
            ArraySize: 1,
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: 0,
            CPUAccessFlags: 0,
            MiscFlags: 0,
            ..desc
        };
        if self.frame.is_none() {
            let mut t = None;
            // SAFETY: `own` describes a valid default-usage 2D texture; out-pointer is live.
            unsafe {
                self.device
                    .CreateTexture2D(&raw const own, None, Some(&raw mut t))
            }
            .ctx("CreateTexture2D(frame)")?;
            self.frame = t;
        }
        if self.want_cpu && self.staging.is_none() {
            let st = D3D11_TEXTURE2D_DESC {
                Usage: D3D11_USAGE_STAGING,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0.cast_unsigned(),
                ..own
            };
            let mut t = None;
            // SAFETY: as above, staging usage with CPU read access.
            unsafe {
                self.device
                    .CreateTexture2D(&raw const st, None, Some(&raw mut t))
            }
            .ctx("CreateTexture2D(staging)")?;
            self.staging = t;
        }
        Ok(())
    }

    fn read_metadata(&mut self, dupl: &IDXGIOutputDuplication, info: &DXGI_OUTDUPL_FRAME_INFO) {
        self.dirty.clear();
        self.moves.clear();
        let bytes = info.TotalMetadataBufferSize as usize;
        if bytes == 0 {
            return;
        }
        let mut required = 0u32;
        self.raw_moves.resize(
            bytes / size_of::<DXGI_OUTDUPL_MOVE_RECT>() + 1,
            DXGI_OUTDUPL_MOVE_RECT::default(),
        );
        let cap = u32::try_from(self.raw_moves.len() * size_of::<DXGI_OUTDUPL_MOVE_RECT>())
            .unwrap_or(u32::MAX);
        // SAFETY: the buffer holds `cap` bytes; `required` is a live out-parameter.
        if unsafe { dupl.GetFrameMoveRects(cap, self.raw_moves.as_mut_ptr(), &raw mut required) }
            .is_ok()
        {
            let n = required as usize / size_of::<DXGI_OUTDUPL_MOVE_RECT>();
            self.moves
                .extend(self.raw_moves[..n].iter().map(|m| MoveRect {
                    source: (m.SourcePoint.x, m.SourcePoint.y),
                    destination: rect(m.DestinationRect),
                }));
        }
        self.raw_dirty
            .resize(bytes / size_of::<RECT>() + 1, RECT::default());
        let cap = u32::try_from(self.raw_dirty.len() * size_of::<RECT>()).unwrap_or(u32::MAX);
        // SAFETY: as above.
        if unsafe { dupl.GetFrameDirtyRects(cap, self.raw_dirty.as_mut_ptr(), &raw mut required) }
            .is_ok()
        {
            let n = required as usize / size_of::<RECT>();
            self.dirty
                .extend(self.raw_dirty[..n].iter().copied().map(rect));
        }
    }

    fn read_cursor(
        &mut self,
        dupl: &IDXGIOutputDuplication,
        info: &DXGI_OUTDUPL_FRAME_INFO,
    ) -> Option<CursorInfo> {
        if info.LastMouseUpdateTime == 0 && info.PointerShapeBufferSize == 0 {
            return None;
        }
        let pos = info.PointerPosition;
        let mut cursor = CursorInfo {
            position: (info.LastMouseUpdateTime != 0).then_some((pos.Position.x, pos.Position.y)),
            visible: pos.Visible.as_bool(),
            shape: None,
        };
        if info.PointerShapeBufferSize > 0 {
            self.shape_buf
                .resize(info.PointerShapeBufferSize as usize, 0);
            let mut required = 0u32;
            let mut si = DXGI_OUTDUPL_POINTER_SHAPE_INFO::default();
            // SAFETY: buffer sized to PointerShapeBufferSize; out-params are live locals.
            let r = unsafe {
                dupl.GetFramePointerShape(
                    info.PointerShapeBufferSize,
                    self.shape_buf.as_mut_ptr().cast(),
                    &raw mut required,
                    &raw mut si,
                )
            };
            if r.is_ok()
                && let Some(kind) = ShapeType::from_raw(si.Type)
            {
                match shape_to_rgba(
                    kind,
                    si.Width,
                    si.Height,
                    si.Pitch,
                    (si.HotSpot.x, si.HotSpot.y),
                    &self.shape_buf,
                ) {
                    Ok(s) => cursor.shape = Some(s),
                    Err(e) => tracing::debug!(error = %e, "cursor shape conversion failed"),
                }
            }
        }
        Some(cursor)
    }

    fn copy_to_cpu(&mut self) -> Result<()> {
        let (Some(frame), Some(staging)) = (&self.frame, &self.staging) else {
            return Ok(());
        };
        // SAFETY: both textures were created with identical descriptions on this device.
        unsafe { self.context.CopyResource(staging, frame) };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: staging texture with CPU read access; `mapped` is a live out-parameter.
        unsafe {
            self.context
                .Map(staging, 0, D3D11_MAP_READ, 0, Some(&raw mut mapped))
        }
        .ctx("Map(staging)")?;
        let row = self.width as usize * 4;
        let pitch = mapped.RowPitch as usize;
        let h = self.height as usize;
        self.cpu.resize(row * h, 0);
        // SAFETY: while mapped, pData points at `RowPitch * Height` readable bytes.
        let src = unsafe { std::slice::from_raw_parts(mapped.pData.cast::<u8>(), pitch * h) };
        for y in 0..h {
            self.cpu[y * row..(y + 1) * row].copy_from_slice(&src[y * pitch..y * pitch + row]);
        }
        // SAFETY: matches the successful Map above.
        unsafe { self.context.Unmap(staging, 0) };
        Ok(())
    }

    fn acquire(&mut self, timeout: Duration) -> Result<Option<(bool, Option<CursorInfo>)>> {
        if self.dupl.is_none() {
            // Lock screen / elevation prompt: attach to that desktop first
            // (works when started by scrin-service; no-op otherwise).
            super::desktop::follow_input_desktop();
            match self.duplicate() {
                Ok(()) => {}
                Err(Error::Os { code, .. })
                    if code == E_ACCESSDENIED.0.cast_unsigned()
                        || code == DXGI_ERROR_ACCESS_LOST.0.cast_unsigned() =>
                {
                    // Secure desktop or mid-switch: try again on the next call.
                    std::thread::sleep(timeout.min(Duration::from_millis(50)));
                    return Ok(None);
                }
                Err(e) => return Err(e),
            }
        }
        let Some(dupl) = self.dupl.clone() else {
            return Ok(None);
        };
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;
        let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: out-parameters are live locals; every success is paired with ReleaseFrame below.
        match unsafe { dupl.AcquireNextFrame(ms, &raw mut info, &raw mut resource) } {
            Ok(()) => {}
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(None),
            Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => {
                tracing::info!("desktop duplication lost; recreating");
                self.dupl = None;
                return Ok(None);
            }
            Err(e) => return Err(super::os_err("AcquireNextFrame", &e)),
        }
        let result = self.consume(&dupl, &info, resource.as_ref());
        // SAFETY: a frame was acquired above and not yet released.
        if let Err(e) = unsafe { dupl.ReleaseFrame() } {
            if e.code() == DXGI_ERROR_ACCESS_LOST {
                self.dupl = None;
            } else {
                return Err(super::os_err("ReleaseFrame", &e));
            }
        }
        result.map(Some)
    }

    fn consume(
        &mut self,
        dupl: &IDXGIOutputDuplication,
        info: &DXGI_OUTDUPL_FRAME_INFO,
        resource: Option<&IDXGIResource>,
    ) -> Result<(bool, Option<CursorInfo>)> {
        let updated = info.LastPresentTime != 0 && info.AccumulatedFrames > 0;
        if updated && let Some(res) = resource {
            let tex: ID3D11Texture2D = res.cast().ctx("IDXGIResource -> ID3D11Texture2D")?;
            self.ensure_textures(&tex)?;
            if let Some(own) = &self.frame {
                // SAFETY: same size and format (own was created from tex's description).
                unsafe { self.context.CopyResource(own, &tex) };
            }
            self.read_metadata(dupl, info);
            if self.want_cpu {
                self.copy_to_cpu()?;
            }
            self.have_image = true;
        }
        let cursor = self.read_cursor(dupl, info);
        Ok((updated, cursor))
    }
}

impl CaptureSource for DxgiCapture {
    fn next_frame(&mut self, timeout: Duration) -> Result<Option<CapturedFrame<'_>>> {
        let Some((updated, cursor)) = self.acquire(timeout)? else {
            return Ok(None);
        };
        if !self.have_image {
            // Cursor-only update before the first image: nothing to show yet.
            return Ok(None);
        }
        if !updated {
            self.dirty.clear();
            self.moves.clear();
        }
        let discontinuity = std::mem::take(&mut self.discontinuity);
        let data = if self.want_cpu {
            PixelData::Cpu(&self.cpu)
        } else {
            match &self.frame {
                Some(t) => PixelData::Texture(t),
                None => return Ok(None),
            }
        };
        Ok(Some(CapturedFrame {
            width: self.width,
            height: self.height,
            stride: if self.want_cpu {
                self.width as usize * 4
            } else {
                0
            },
            data,
            image_updated: updated,
            dirty_rects: &self.dirty,
            move_rects: &self.moves,
            pts_us: u64::try_from(self.start.elapsed().as_micros()).unwrap_or(u64::MAX),
            cursor,
            discontinuity,
        }))
    }

    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        list_displays()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_displays_with_one_primary() {
        let displays = match list_displays() {
            Ok(d) => d,
            Err(e) if super::super::headless() => return eprintln!("skipped: {e}"),
            Err(e) => panic!("list_displays: {e}"),
        };
        assert!(
            !displays.is_empty(),
            "an interactive session has at least one display"
        );
        assert_eq!(displays.iter().filter(|d| d.primary).count(), 1);
        for d in &displays {
            assert!(d.bounds.width() > 0 && d.bounds.height() > 0, "{d:?}");
            assert!(d.name.starts_with(r"\\.\"), "{d:?}");
        }
        let desk = virtual_desktop().expect("virtual desktop");
        assert!(desk.width() >= displays[0].bounds.width());
    }

    #[test]
    fn captures_a_cpu_frame_from_the_primary_display() {
        let mut cap = match DxgiCapture::primary(true) {
            Ok(c) => c,
            Err(e) if super::super::headless() => return eprintln!("skipped: {e}"),
            Err(e) => panic!("open: {e}"),
        };
        // The first AcquireNextFrame after duplication always returns the full desktop image.
        let mut got = None;
        for _ in 0..20 {
            if let Some(f) = cap
                .next_frame(Duration::from_millis(100))
                .expect("next_frame")
            {
                got = Some((
                    f.width,
                    f.height,
                    f.stride,
                    f.image_updated,
                    match f.data {
                        PixelData::Cpu(b) => b.len(),
                        PixelData::Texture(_) => 0,
                    },
                ));
                break;
            }
        }
        let (w, h, stride, updated, len) = got.expect("a frame within 2 s");
        assert!(updated);
        assert_eq!(stride, w as usize * 4);
        assert_eq!(len, stride * h as usize);
        assert_eq!(
            (w, h),
            (cap.display().bounds.width(), cap.display().bounds.height())
        );
    }
}
