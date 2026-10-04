//! One-time connection codes.
//!
//! 8 symbols from a 30-symbol alphabet without look-alikes (no I, L, O, U, 0, 1):
//! ~39 bits. Strength comes from the PAKE (one online guess per attempt) plus
//! server-side attempt limits, not from the code length alone.

use std::time::{Duration, Instant};

use zeroize::Zeroizing;

use crate::{Error, Result, random_bytes};

pub const ALPHABET: &[u8; 30] = b"ABCDEFGHJKMNPQRSTVWXYZ23456789";
pub const LEN: usize = 8;
pub const DEFAULT_TTL: Duration = Duration::from_secs(600);

/// A code the host displays. Consumed by the first pairing attempt, right or wrong.
#[derive(Clone)]
pub struct OneTimeCode {
    code: Zeroizing<String>,
    expires: Instant,
}

impl OneTimeCode {
    pub fn generate() -> Result<Self> {
        Self::generate_with_ttl(DEFAULT_TTL)
    }

    pub fn generate_with_ttl(ttl: Duration) -> Result<Self> {
        let mut out = String::with_capacity(LEN);
        // Rejection sampling: 240 = 8 * 30 is the largest multiple of 30 below 256,
        // so every symbol is equally likely.
        while out.len() < LEN {
            for b in random_bytes::<16>()? {
                if b < 240 && out.len() < LEN {
                    out.push(char::from(ALPHABET[usize::from(b % 30)]));
                }
            }
        }
        Ok(Self {
            code: Zeroizing::new(out),
            expires: Instant::now() + ttl,
        })
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.code
    }

    /// Display form `ABCD-EFGH`.
    #[must_use]
    pub fn display(&self) -> String {
        format!("{}-{}", &self.code[..4], &self.code[4..])
    }

    #[must_use]
    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.expires
    }

    #[must_use]
    pub fn remaining(&self) -> Duration {
        self.expires.saturating_duration_since(Instant::now())
    }
}

impl std::fmt::Debug for OneTimeCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OneTimeCode(<redacted>)")
    }
}

/// Normalises user input: uppercase, strips spaces and dashes, maps common
/// look-alikes (O→0 is NOT valid, so O/0 and I/1/L map to nothing and fail).
pub fn normalize(input: &str) -> Result<Zeroizing<String>> {
    let s: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if s.len() != LEN {
        return Err(Error::Malformed("code length"));
    }
    if !s.bytes().all(|b| ALPHABET.contains(&b)) {
        return Err(Error::Malformed("code alphabet"));
    }
    Ok(Zeroizing::new(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_codes_are_valid_and_distinct() {
        let a = OneTimeCode::generate().expect("rng");
        let b = OneTimeCode::generate().expect("rng");
        assert_eq!(a.as_str().len(), LEN);
        assert!(normalize(a.as_str()).is_ok());
        assert_ne!(a.as_str(), b.as_str());
    }

    #[test]
    fn normalize_accepts_display_form_and_lowercase() {
        let c = OneTimeCode::generate().expect("rng");
        let typed = c.display().to_lowercase();
        assert_eq!(normalize(&typed).expect("valid").as_str(), c.as_str());
    }

    #[test]
    fn normalize_rejects_lookalikes_and_bad_length() {
        assert!(normalize("ABCD-EFG0").is_err());
        assert!(normalize("ABCD-EFGI").is_err());
        assert!(normalize("ABC").is_err());
    }

    #[test]
    fn ttl_expires() {
        let c = OneTimeCode::generate_with_ttl(Duration::ZERO).expect("rng");
        assert!(c.is_expired());
    }

    #[test]
    fn debug_never_prints_code() {
        let c = OneTimeCode::generate().expect("rng");
        assert!(!format!("{c:?}").contains(c.as_str()));
    }
}
