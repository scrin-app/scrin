//! Where the engine keeps secrets: the identity seed and the sealed trust store.
//!
//! [`SecretStore`] is a tiny named-blob store. On Windows [`DpapiStore`] seals
//! every blob with DPAPI (`CryptProtectData`, current user scope) before it
//! touches disk, so the seed is never stored in the clear. [`FileStore`] keeps
//! blobs as plain files and exists for tests and non-Windows development.

use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use crate::{EngineError, Result};

/// Named blobs. Names are short ASCII identifiers chosen by the engine.
pub trait SecretStore: Send + Sync + std::fmt::Debug + 'static {
    /// The blob, or `None` when it was never stored.
    fn load(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>>;
    fn store(&self, name: &str, value: &[u8]) -> Result<()>;
    fn remove(&self, name: &str) -> Result<()>;
}

fn check_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(EngineError::Invalid("secret name"))
    }
}

/// Writes `bytes` to `path` via a temporary sibling and a rename, so a crash
/// never leaves a half-written seed behind.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn read_opt(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn remove_opt(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Plain files in a directory. **Not** protected at rest: tests and dev only.
#[derive(Debug, Clone)]
pub struct FileStore {
    dir: PathBuf,
}

impl FileStore {
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, name: &str) -> Result<PathBuf> {
        check_name(name)?;
        Ok(self.dir.join(format!("{name}.bin")))
    }
}

impl SecretStore for FileStore {
    fn load(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
        Ok(read_opt(&self.path(name)?)?.map(Zeroizing::new))
    }

    fn store(&self, name: &str, value: &[u8]) -> Result<()> {
        write_atomic(&self.path(name)?, value)
    }

    fn remove(&self, name: &str) -> Result<()> {
        remove_opt(&self.path(name)?)
    }
}

/// DPAPI-sealed files (Windows, current-user scope). Each blob is bound to an
/// application entropy string so another program running as the same user
/// cannot unseal it with a plain `CryptUnprotectData` call.
#[derive(Debug, Clone)]
pub struct DpapiStore {
    dir: PathBuf,
}

/// Extra DPAPI entropy. Not a secret; it scopes blobs to scrin.
const DPAPI_ENTROPY: &[u8] = b"scrin/1 device secret store";

impl DpapiStore {
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, name: &str) -> Result<PathBuf> {
        check_name(name)?;
        Ok(self.dir.join(format!("{name}.dpapi")))
    }
}

impl SecretStore for DpapiStore {
    fn load(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
        match read_opt(&self.path(name)?)? {
            Some(sealed) => dpapi::unprotect(&sealed, DPAPI_ENTROPY).map(Some),
            None => Ok(None),
        }
    }

    fn store(&self, name: &str, value: &[u8]) -> Result<()> {
        let sealed = dpapi::protect(value, DPAPI_ENTROPY)?;
        write_atomic(&self.path(name)?, &sealed)
    }

    fn remove(&self, name: &str) -> Result<()> {
        remove_opt(&self.path(name)?)
    }
}

/// The platform's protected store: DPAPI on Windows, plain files elsewhere
/// (development only — Android and the web keep their own stores).
#[must_use]
pub fn platform_store(dir: impl Into<PathBuf>) -> Box<dyn SecretStore> {
    if cfg!(windows) {
        Box::new(DpapiStore::new(dir))
    } else {
        Box::new(FileStore::new(dir))
    }
}

#[cfg(windows)]
mod dpapi {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };
    use windows::core::PCWSTR;
    use zeroize::Zeroizing;

    use crate::{EngineError, Result};

    fn blob(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB> {
        Ok(CRYPT_INTEGER_BLOB {
            cbData: u32::try_from(bytes.len()).map_err(|_| EngineError::Invalid("blob size"))?,
            // DPAPI takes a mutable pointer but only reads input blobs.
            pbData: bytes.as_ptr().cast_mut(),
        })
    }

    /// Copies the `LocalAlloc`'d output, wipes it, and frees it.
    fn take_output(out: &CRYPT_INTEGER_BLOB) -> Zeroizing<Vec<u8>> {
        let len = out.cbData as usize;
        let copy = if out.pbData.is_null() || len == 0 {
            Vec::new()
        } else {
            // SAFETY: after a successful CryptProtectData/CryptUnprotectData
            // call `pbData` points at `cbData` initialised bytes owned by us.
            unsafe { std::slice::from_raw_parts(out.pbData, len) }.to_vec()
        };
        if !out.pbData.is_null() {
            // SAFETY: the same buffer as above; we own it, nobody else reads
            // it, and wiping before LocalFree keeps plaintext off the heap.
            unsafe { std::ptr::write_bytes(out.pbData, 0, len) };
            // SAFETY: DPAPI allocates output with LocalAlloc and documents
            // LocalFree as the way to release it; freed exactly once here.
            unsafe { LocalFree(Some(HLOCAL(out.pbData.cast()))) };
        }
        Zeroizing::new(copy)
    }

    pub(super) fn protect(plain: &[u8], entropy: &[u8]) -> Result<Vec<u8>> {
        let input = blob(plain)?;
        let ent = blob(entropy)?;
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: `input` and `ent` borrow slices that outlive the call; `out`
        // is a valid out-pointer; no prompt struct; UI is forbidden.
        unsafe {
            CryptProtectData(
                &raw const input,
                PCWSTR::null(),
                Some(&raw const ent),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &raw mut out,
            )
        }
        .map_err(|e| EngineError::Secret(format!("CryptProtectData: {e}")))?;
        Ok(take_output(&out).to_vec())
    }

    pub(super) fn unprotect(sealed: &[u8], entropy: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        let input = blob(sealed)?;
        let ent = blob(entropy)?;
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: as in `protect`; the description out-pointer is not requested.
        unsafe {
            CryptUnprotectData(
                &raw const input,
                None,
                Some(&raw const ent),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &raw mut out,
            )
        }
        .map_err(|e| EngineError::Secret(format!("CryptUnprotectData: {e}")))?;
        Ok(take_output(&out))
    }
}

#[cfg(not(windows))]
mod dpapi {
    use zeroize::Zeroizing;

    use crate::{EngineError, Result};

    pub(super) fn protect(_plain: &[u8], _entropy: &[u8]) -> Result<Vec<u8>> {
        Err(EngineError::Secret("DPAPI is Windows-only".into()))
    }

    pub(super) fn unprotect(_sealed: &[u8], _entropy: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        Err(EngineError::Secret("DPAPI is Windows-only".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn temp_dir(tag: &str) -> PathBuf {
        let mut n = [0u8; 8];
        getrandom::fill(&mut n).expect("rng");
        std::env::temp_dir().join(format!("scrin-engine-{tag}-{}", u64::from_le_bytes(n)))
    }

    #[test]
    fn file_store_round_trip_and_remove() {
        let dir = temp_dir("file");
        let s = FileStore::new(&dir);
        assert!(s.load("seed").expect("load").is_none());
        s.store("seed", b"abc").expect("store");
        assert_eq!(
            s.load("seed").expect("load").expect("some").as_slice(),
            b"abc"
        );
        s.remove("seed").expect("remove");
        assert!(s.load("seed").expect("load").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn names_are_restricted() {
        let s = FileStore::new(temp_dir("names"));
        assert!(s.store("../evil", b"x").is_err());
        assert!(s.store("", b"x").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_seals_and_unseals() {
        let dir = temp_dir("dpapi");
        let s = DpapiStore::new(&dir);
        s.store("seed", &[7u8; 32]).expect("store");
        let raw = std::fs::read(dir.join("seed.dpapi")).expect("file");
        assert!(
            !raw.windows(32).any(|w| w == [7u8; 32]),
            "plaintext on disk"
        );
        assert_eq!(
            s.load("seed").expect("load").expect("some").as_slice(),
            &[7u8; 32]
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
