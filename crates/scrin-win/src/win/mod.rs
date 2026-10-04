//! Windows implementations of the portable traits.

pub mod audio;
pub mod capture_dxgi;
pub mod clipboard;
pub mod decode_openh264;
pub mod desktop;
pub mod encode_mf;
pub mod encode_openh264;
pub mod input;
pub mod sas_client;

use crate::{Error, Result, VideoEncoder};
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

/// Builds an [`Error::Os`] from a windows-rs error.
pub(crate) fn os_err(context: &'static str, e: &windows::core::Error) -> Error {
    Error::Os {
        context,
        code: e.code().0.cast_unsigned(),
        message: e.message(),
    }
}

/// Attaches an API name to a windows-rs result.
pub(crate) trait OsContext<T> {
    fn ctx(self, context: &'static str) -> Result<T>;
}

impl<T> OsContext<T> for windows::core::Result<T> {
    fn ctx(self, context: &'static str) -> Result<T> {
        self.map_err(|e| os_err(context, &e))
    }
}

/// Initialises COM (multithreaded apartment) on the current thread for the guard's lifetime.
#[derive(Debug)]
pub(crate) struct ComGuard {
    owns: bool,
}

impl ComGuard {
    pub(crate) fn mta() -> Result<Self> {
        // SAFETY: CoInitializeEx has no pointer arguments; balanced by CoUninitialize in Drop
        // only when this call succeeded.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if hr.is_ok() {
            Ok(Self { owns: true })
        } else if hr == RPC_E_CHANGED_MODE {
            // The thread is already an STA; COM is usable, just not ours to uninitialise.
            Ok(Self { owns: false })
        } else {
            Err(os_err(
                "CoInitializeEx",
                &windows::core::Error::from_hresult(hr),
            ))
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.owns {
            // SAFETY: paired with the successful CoInitializeEx in `mta` on this same thread
            // (ComGuard is !Send because the struct holding it holds COM pointers).
            unsafe { CoUninitialize() };
        }
    }
}

/// Encoder settings shared by all encoders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderSettings {
    /// Output width (rounded down to even).
    pub width: u32,
    /// Output height (rounded down to even).
    pub height: u32,
    /// Target frame rate.
    pub fps: u32,
    /// Target bitrate in bits per second.
    pub bitrate: u32,
}

/// Creates the best available H.264 encoder: a hardware Media Foundation MFT when present,
/// otherwise openh264.
pub fn best_encoder(settings: EncoderSettings) -> Result<Box<dyn VideoEncoder>> {
    match encode_mf::MfEncoder::new_hardware(settings) {
        Ok(e) => Ok(Box::new(e)),
        Err(e) => {
            tracing::info!(error = %e, "no usable hardware H.264 MFT; falling back to openh264");
            Ok(Box::new(encode_openh264::OpenH264Encoder::new(settings)?))
        }
    }
}

/// Whether Windows-API tests may tolerate a missing desktop/GPU/audio device (CI runners).
#[cfg(test)]
pub(crate) fn headless() -> bool {
    std::env::var_os("CI").is_some()
}
