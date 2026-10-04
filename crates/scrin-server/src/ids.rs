//! scrin IDs: random 9-digit numbers bound to a device key.
//!
//! IDs are drawn uniformly from `100_000_000..=999_999_999` with the OS CSPRNG
//! (rejection sampling, no modulo bias). They are never sequential, so knowing
//! one ID says nothing about any other.

/// Smallest valid ID (no leading zero, so it always prints as 9 digits).
pub const ID_MIN: u64 = 100_000_000;
/// Largest valid ID.
pub const ID_MAX: u64 = 999_999_999;

const SPAN: u64 = ID_MAX - ID_MIN + 1;
/// Largest multiple of [`SPAN`] that fits in `2^32`; samples at or above it
/// are rejected so every ID is equally likely.
const ACCEPT_BELOW: u64 = (1u64 << 32) - ((1u64 << 32) % SPAN);

/// A fresh uniformly random ID. Uniqueness is the caller's job (the store).
pub fn random_id() -> Result<u64, getrandom::Error> {
    loop {
        let mut b = [0u8; 4];
        getrandom::fill(&mut b)?;
        let v = u64::from(u32::from_be_bytes(b));
        if v < ACCEPT_BELOW {
            return Ok(ID_MIN + v % SPAN);
        }
    }
}

#[must_use]
pub fn is_valid_id(id: u64) -> bool {
    (ID_MIN..=ID_MAX).contains(&id)
}

/// Parses `123456789`, `123 456 789` or `123-456-789`.
#[must_use]
pub fn parse_id(s: &str) -> Option<u64> {
    let digits: String = s.chars().filter(|c| !matches!(c, ' ' | '-')).collect();
    if digits.len() != 9 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().filter(|id| is_valid_id(*id))
}

/// Canonical wire form: 9 digits, no separators.
#[must_use]
pub fn format_id(id: u64) -> String {
    format!("{id:09}")
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn random_ids_are_nine_digits() {
        for _ in 0..10_000 {
            let id = random_id().expect("rng");
            assert!(is_valid_id(id), "{id}");
            assert_eq!(format_id(id).len(), 9);
        }
    }

    #[test]
    fn hundred_thousand_ids_do_not_collide_and_are_not_sequential() {
        let ids: Vec<u64> = (0..100_000).map(|_| random_id().expect("rng")).collect();
        let unique: HashSet<_> = ids.iter().copied().collect();
        // Birthday bound for 1e5 draws from 9e8: expected ~5.6 collisions. The
        // store rejects duplicates, so allow a handful but no systematic repeats.
        assert!(
            unique.len() >= 99_970,
            "too many collisions: {}",
            100_000 - unique.len()
        );
        let sequential = ids.windows(2).filter(|w| w[1] == w[0] + 1).count();
        assert!(sequential < 3, "sequential pairs: {sequential}");
    }

    #[test]
    fn random_ids_cover_the_whole_range() {
        // 10 buckets of 90M each; a uniform sample of 20k fills each with ~2000.
        let mut buckets = [0u32; 10];
        for _ in 0..20_000 {
            let id = random_id().expect("rng");
            let b = usize::try_from((id - ID_MIN) / (SPAN / 10)).expect("bucket");
            buckets[b] += 1;
        }
        for (i, n) in buckets.iter().enumerate() {
            assert!((1_500..2_500).contains(n), "bucket {i} = {n}");
        }
    }

    #[test]
    fn parse_accepts_grouping_and_rejects_garbage() {
        assert_eq!(parse_id("123456789"), Some(123_456_789));
        assert_eq!(parse_id("123 456 789"), Some(123_456_789));
        assert_eq!(parse_id("123-456-789"), Some(123_456_789));
        assert_eq!(parse_id("012345678"), None);
        assert_eq!(parse_id("12345678"), None);
        assert_eq!(parse_id("1234567890"), None);
        assert_eq!(parse_id("12345678a"), None);
        assert_eq!(parse_id("+12345678"), None);
    }
}
