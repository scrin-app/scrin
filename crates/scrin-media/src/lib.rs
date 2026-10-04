//! Codec traits, packetizer, frame pacing and bandwidth estimation for scrin.
//!
//! Pure Rust, no OS APIs:
//! * [`fec`] – frame sharding + Reed-Solomon parity, 16-byte shard header, reassembly.
//! * [`bwe`] – GCC-style delay + loss bandwidth estimator.
//! * [`adapt`] – target bitrate → bitrate / fps / resolution with hysteresis.
//! * [`pacing`] – capture pacer and latest-frame-wins present queue.
//! * [`clock`] – NTP-style offset / RTT with a min-RTT filter.

pub mod adapt;
pub mod bwe;
pub mod clock;
pub mod fec;
pub mod pacing;
