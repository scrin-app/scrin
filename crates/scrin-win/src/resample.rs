//! Audio format conversion to the wire format: interleaved stereo i16 at 48 kHz in 10 ms frames.
//!
//! [`LinearResampler`] is a streaming linear-interpolation resampler (phase carried across calls,
//! no allocation per call once warmed up). [`Framer`] cuts the resampled stream into fixed
//! 480-sample frames. Linear interpolation is adequate for the common 44.1 ↔ 48 kHz case of a
//! remote-desktop audio stream; it is not a mastering-grade resampler.

use crate::{AUDIO_CHANNELS, AUDIO_FRAME_SAMPLES};

/// Converts a float sample (nominal −1..=1) to i16 with clipping.
#[must_use]
pub fn f32_to_i16(s: f32) -> i16 {
    let scaled = (s * 32_767.0).round().clamp(-32_768.0, 32_767.0);
    // The clamp keeps the value inside i16's range, so the cast is exact.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "clamped to the i16 range above"
    )]
    let v = scaled as i16;
    v
}

/// Maps any channel count to stereo (mono duplicates; >2 keeps front left/right).
pub fn to_stereo(frame: &[f32], out: &mut [f32; 2]) {
    match frame {
        [] => *out = [0.0, 0.0],
        [m] => *out = [*m, *m],
        [l, r, ..] => *out = [*l, *r],
    }
}

/// Streaming linear resampler for interleaved stereo f32.
#[derive(Debug, Clone)]
pub struct LinearResampler {
    in_rate: u32,
    out_rate: u32,
    /// Position of the next output sample, in input samples, relative to `prev`.
    phase: f64,
    /// Last input frame of the previous call (interpolation needs one frame of history).
    prev: [f32; 2],
    primed: bool,
}

impl LinearResampler {
    /// Creates a resampler; rates must be non-zero.
    #[must_use]
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        Self {
            in_rate: in_rate.max(1),
            out_rate: out_rate.max(1),
            phase: 0.0,
            prev: [0.0; 2],
            primed: false,
        }
    }

    /// Input rate in Hz.
    #[must_use]
    pub fn in_rate(&self) -> u32 {
        self.in_rate
    }

    /// Resamples interleaved stereo `input`, appending interleaved stereo to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        let frames = input.len() / 2;
        if frames == 0 {
            return;
        }
        if self.in_rate == self.out_rate {
            out.extend_from_slice(&input[..frames * 2]);
            return;
        }
        if !self.primed {
            self.prev = [input[0], input[1]];
            self.primed = true;
            // Phase 1.0 = input frame 0: the first output sample lands exactly on it.
            self.phase = 1.0;
        }
        let step = f64::from(self.in_rate) / f64::from(self.out_rate);
        // Sample at index -1 is `prev`, index k (0..frames) is input frame k.
        let at = |k: isize| -> [f32; 2] {
            if k < 0 {
                self.prev
            } else {
                let k = k.unsigned_abs();
                [input[k * 2], input[k * 2 + 1]]
            }
        };
        // phase is measured from `prev` (index -1).
        #[expect(clippy::cast_precision_loss, reason = "frame counts are small")]
        let limit = frames as f64;
        while self.phase < limit {
            // Truncation is the floor here: phase is non-negative.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "phase < frames, non-negative"
            )]
            let i = self.phase.floor() as isize - 1;
            #[expect(clippy::cast_possible_truncation, reason = "fraction in 0..1")]
            let frac = (self.phase - self.phase.floor()) as f32;
            let a = at(i);
            let b = at(i + 1);
            out.push(a[0] + (b[0] - a[0]) * frac);
            out.push(a[1] + (b[1] - a[1]) * frac);
            self.phase += step;
        }
        self.phase -= limit;
        self.prev = [input[(frames - 1) * 2], input[(frames - 1) * 2 + 1]];
    }
}

/// Accumulates interleaved stereo i16 and yields exact 10 ms frames.
#[derive(Debug, Default, Clone)]
pub struct Framer {
    buf: Vec<i16>,
    read: usize,
}

impl Framer {
    /// Samples (both channels) in one frame.
    pub const FRAME_LEN: usize = AUDIO_FRAME_SAMPLES * AUDIO_CHANNELS;

    /// Appends float samples, converting to i16.
    pub fn push_f32(&mut self, samples: &[f32]) {
        self.compact();
        self.buf.extend(samples.iter().map(|&s| f32_to_i16(s)));
    }

    /// Appends silence for `frames` stereo frames.
    pub fn push_silence(&mut self, frames: usize) {
        self.compact();
        self.buf.resize(self.buf.len() + frames * AUDIO_CHANNELS, 0);
    }

    /// The next full frame, if available.
    pub fn pop(&mut self) -> Option<&[i16]> {
        if self.buf.len() - self.read < Self::FRAME_LEN {
            return None;
        }
        let start = self.read;
        self.read += Self::FRAME_LEN;
        Some(&self.buf[start..start + Self::FRAME_LEN])
    }

    /// Buffered stereo frames not yet popped.
    #[must_use]
    pub fn pending_frames(&self) -> usize {
        (self.buf.len() - self.read) / AUDIO_CHANNELS
    }

    fn compact(&mut self) {
        if self.read > 0 {
            self.buf.drain(..self.read);
            self.read = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, hz: f32, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|n| {
                #[expect(clippy::cast_precision_loss, reason = "test signal")]
                let t = n as f32 / rate as f32;
                let s = (t * hz * std::f32::consts::TAU).sin() * 0.5;
                [s, s]
            })
            .collect()
    }

    #[test]
    fn f32_to_i16_clips_and_rounds() {
        assert_eq!(f32_to_i16(0.0), 0);
        assert_eq!(f32_to_i16(1.0), 32_767);
        assert_eq!(f32_to_i16(-1.0), -32_767);
        assert_eq!(f32_to_i16(2.0), 32_767);
        assert_eq!(f32_to_i16(-2.0), -32_768);
        assert_eq!(f32_to_i16(0.5), 16_384);
    }

    #[test]
    fn stereo_mapping() {
        let mut o = [0.0; 2];
        to_stereo(&[0.25], &mut o);
        assert_eq!(o, [0.25, 0.25]);
        to_stereo(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6], &mut o);
        assert_eq!(o, [0.1, 0.2]);
    }

    #[test]
    fn passthrough_at_equal_rates() {
        let mut r = LinearResampler::new(48_000, 48_000);
        let input = sine(48_000, 1000.0, 100);
        let mut out = Vec::new();
        r.process(&input, &mut out);
        assert_eq!(out, input);
    }

    #[test]
    fn output_length_tracks_ratio_across_chunks() {
        for (inr, outr) in [(44_100, 48_000), (96_000, 48_000), (32_000, 48_000)] {
            let mut r = LinearResampler::new(inr, outr);
            let input = sine(inr, 440.0, inr as usize); // 1 s
            let mut out = Vec::new();
            // Odd chunk sizes exercise the phase carry.
            for chunk in input.chunks(2 * 333) {
                r.process(chunk, &mut out);
            }
            let got = out.len() / 2;
            let want = outr as usize;
            assert!(
                got.abs_diff(want) <= 2,
                "{inr}->{outr}: {got} frames, want {want}"
            );
        }
    }

    #[test]
    fn resampled_sine_matches_reference() {
        let (inr, outr, hz) = (44_100, 48_000, 1_000.0);
        let mut r = LinearResampler::new(inr, outr);
        let mut out = Vec::new();
        for chunk in sine(inr, hz, 4_410).chunks(2 * 441) {
            r.process(chunk, &mut out);
        }
        let reference = sine(outr, hz, out.len() / 2);
        let max_err = out
            .iter()
            .zip(&reference)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        // Linear interpolation error for a 1 kHz tone at 44.1 kHz is < 1.3 % of full scale.
        assert!(max_err < 0.013, "max error {max_err}");
    }

    #[test]
    fn framer_emits_exact_10ms_frames() {
        let mut f = Framer::default();
        f.push_f32(&vec![0.5; 700 * 2]);
        let first = f.pop().expect("one frame").to_vec();
        assert_eq!(first.len(), 960);
        assert!(first.iter().all(|&s| s == 16_384));
        assert!(f.pop().is_none());
        assert_eq!(f.pending_frames(), 220);
        f.push_silence(260);
        assert_eq!(f.pop().map(<[i16]>::len), Some(960));
        assert_eq!(f.pending_frames(), 0);
    }
}
