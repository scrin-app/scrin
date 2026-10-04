//! Inner end-to-end channel for connections bridged by the browser gateway.
//!
//! The gateway terminates the browser's WebTransport TLS, so the QUIC layer is
//! not end-to-end on that path. After SPAKE2 (or trusted auth) the two ends
//! derive per-direction, per-lane keys and seal every frame/datagram with
//! ChaCha20-Poly1305. The gateway only ever sees ciphertext.
//!
//! Nonce = 4 zero bytes || u64 counter (big-endian). The counter is sent in the
//! clear as an 8-byte prefix. Ordered lanes (streams) require exactly the next
//! counter; unordered lanes (datagrams) accept any counter inside a 64-wide
//! sliding window, once.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use zeroize::Zeroizing;

use crate::{Error, Result};

/// Which end of the session this is; decides which derived key seals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Host,
    Controller,
}

/// Whether a lane delivers in order (stream) or not (datagrams).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ordering {
    Ordered,
    Unordered,
}

const WINDOW: u64 = 64;

/// One direction of one lane.
pub struct Sealer {
    aead: ChaCha20Poly1305,
    counter: u64,
}

pub struct Opener {
    aead: ChaCha20Poly1305,
    ordering: Ordering,
    /// Highest counter accepted + 1 (0 = none yet).
    next: u64,
    /// Bit i set = counter (next - 1 - i) already seen (unordered only).
    seen: u64,
}

impl std::fmt::Debug for Sealer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sealer")
            .field("counter", &self.counter)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for Opener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Opener")
            .field("ordering", &self.ordering)
            .finish_non_exhaustive()
    }
}

/// Derives the sealer/opener pair for `lane` from a 32-byte session secret
/// (e.g. `Paired::export("scrin gateway channel v1")`).
#[must_use]
pub fn lane(secret: &[u8; 32], side: Side, lane: u32, ordering: Ordering) -> (Sealer, Opener) {
    let key = |dir: &[u8]| -> Zeroizing<[u8; 32]> {
        let mut h = blake3::Hasher::new_keyed(secret);
        h.update(b"scrin channel v1/");
        h.update(dir);
        h.update(&lane.to_be_bytes());
        Zeroizing::new(*h.finalize().as_bytes())
    };
    let h2c = key(b"host->controller");
    let c2h = key(b"controller->host");
    let (send, recv) = match side {
        Side::Host => (h2c, c2h),
        Side::Controller => (c2h, h2c),
    };
    // A 32-byte slice always satisfies the key length.
    #[allow(clippy::expect_used)]
    let aead = |k: &[u8; 32]| ChaCha20Poly1305::new_from_slice(k).expect("32-byte key");
    (
        Sealer {
            aead: aead(&send),
            counter: 0,
        },
        Opener {
            aead: aead(&recv),
            ordering,
            next: 0,
            seen: 0,
        },
    )
}

fn nonce(counter: u64) -> Nonce {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&counter.to_be_bytes());
    Nonce::from(n)
}

impl Sealer {
    /// `counter(8) || ciphertext+tag`.
    pub fn seal(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let c = self.counter;
        self.counter = c
            .checked_add(1)
            .ok_or(Error::Malformed("channel counter exhausted"))?;
        let ct = self
            .aead
            .encrypt(&nonce(c), plaintext)
            .map_err(|_| Error::Malformed("seal"))?;
        let mut out = Vec::with_capacity(8 + ct.len());
        out.extend_from_slice(&c.to_be_bytes());
        out.extend_from_slice(&ct);
        Ok(out)
    }
}

impl Opener {
    pub fn open(&mut self, sealed: &[u8]) -> Result<Vec<u8>> {
        if sealed.len() < 8 + 16 {
            return Err(Error::Malformed("sealed frame too short"));
        }
        let (prefix, ct) = sealed.split_at(8);
        let mut cb = [0u8; 8];
        cb.copy_from_slice(prefix);
        let c = u64::from_be_bytes(cb);
        self.check(c)?;
        let pt = self
            .aead
            .decrypt(&nonce(c), ct)
            .map_err(|_| Error::PairingFailed)?;
        self.commit(c);
        Ok(pt)
    }

    fn check(&self, c: u64) -> Result<()> {
        match self.ordering {
            Ordering::Ordered if c != self.next => Err(Error::Malformed("out-of-order frame")),
            Ordering::Ordered => Ok(()),
            Ordering::Unordered => {
                if c >= self.next {
                    return Ok(());
                }
                let age = self.next - 1 - c;
                if age >= WINDOW || self.seen & (1 << age) != 0 {
                    Err(Error::Malformed("replayed or stale datagram"))
                } else {
                    Ok(())
                }
            }
        }
    }

    fn commit(&mut self, c: u64) {
        if c >= self.next {
            let shift = c + 1 - self.next;
            self.seen = if shift >= WINDOW {
                0
            } else {
                self.seen << shift
            };
            self.seen |= 1;
            self.next = c + 1;
        } else {
            self.seen |= 1 << (self.next - 1 - c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: [u8; 32] = [42; 32];

    #[test]
    fn ordered_round_trip_both_directions() {
        let (mut hs, mut ho) = lane(&S, Side::Host, 1, Ordering::Ordered);
        let (mut cs, mut co) = lane(&S, Side::Controller, 1, Ordering::Ordered);
        for i in 0..5u8 {
            assert_eq!(
                co.open(&hs.seal(&[i; 10]).expect("seal")).expect("open"),
                [i; 10]
            );
            assert_eq!(
                ho.open(&cs.seal(&[i; 3]).expect("seal")).expect("open"),
                [i; 3]
            );
        }
    }

    #[test]
    fn ordered_rejects_replay_and_reorder() {
        let (mut hs, _) = lane(&S, Side::Host, 0, Ordering::Ordered);
        let (_, mut co) = lane(&S, Side::Controller, 0, Ordering::Ordered);
        let a = hs.seal(b"a").expect("seal");
        let b = hs.seal(b"b").expect("seal");
        assert!(co.open(&b).is_err());
        co.open(&a).expect("first");
        assert!(co.open(&a).is_err());
        co.open(&b).expect("second");
    }

    #[test]
    fn unordered_window_accepts_reorder_once() {
        let (mut hs, _) = lane(&S, Side::Host, 2, Ordering::Unordered);
        let (_, mut co) = lane(&S, Side::Controller, 2, Ordering::Unordered);
        let frames: Vec<_> = (0..70u8).map(|i| hs.seal(&[i]).expect("seal")).collect();
        co.open(&frames[5]).expect("5");
        co.open(&frames[2]).expect("2 late but in window");
        assert!(co.open(&frames[2]).is_err(), "replay");
        co.open(&frames[69]).expect("69");
        assert!(co.open(&frames[3]).is_err(), "outside window");
    }

    #[test]
    fn wrong_lane_or_key_or_tamper_fails() {
        let (mut hs, _) = lane(&S, Side::Host, 1, Ordering::Ordered);
        let (_, mut other_lane) = lane(&S, Side::Controller, 2, Ordering::Ordered);
        let (_, mut other_key) = lane(&[1; 32], Side::Controller, 1, Ordering::Ordered);
        let (_, mut same_side) = lane(&S, Side::Host, 1, Ordering::Ordered);
        let f = hs.seal(b"secret").expect("seal");
        assert!(other_lane.open(&f).is_err());
        assert!(other_key.open(&f).is_err());
        assert!(same_side.open(&f).is_err(), "reflection must fail");
        let (_, mut ok) = lane(&S, Side::Controller, 1, Ordering::Ordered);
        let mut t = f.clone();
        let last = t.len() - 1;
        t[last] ^= 1;
        assert!(ok.open(&t).is_err());
        assert!(ok.open(&f).is_ok(), "failed open must not advance state");
    }
}
