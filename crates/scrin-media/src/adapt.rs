//! Maps a target bitrate to encoder settings (bitrate, fps, resolution scale).
//!
//! The controller walks a ladder of `(fps, scale)` levels ordered from best to
//! worst. Each level has a bitrate *floor* proportional to the pixels it
//! encodes per second (`floor_bps_at_full × scale² × fps / max_fps`). The
//! bitrate is lowered first; only when the target falls below the floor of the
//! current level does the controller step to the next level.
//!
//! * [`Mode::Quality`] (IT support): keep resolution, drop fps first
//!   (60 → 30 → 15), then resolution (1.0 → 0.75 → 0.5).
//! * [`Mode::Latency`] (gaming): keep fps, drop resolution first, then fps.
//!
//! Stepping **down** is immediate. Stepping **up** happens one level at a time
//! and only after the target has stayed above the better level's floor (plus a
//! margin) for [`AdaptConfig::upgrade_hold_us`], so the stream does not flap.

/// Which dimension to sacrifice first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Keep resolution sharp; reduce frame rate first.
    Quality,
    /// Keep frame rate high; reduce resolution first.
    Latency,
}

/// Settings handed to the encoder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EncoderSettings {
    /// Encoder target bitrate.
    pub bitrate_bps: u32,
    /// Frames per second to capture/encode.
    pub fps: u32,
    /// Linear resolution scale relative to the source (1.0, 0.75, 0.5).
    pub scale: f32,
}

/// Controller tuning.
#[derive(Debug, Clone, PartialEq)]
pub struct AdaptConfig {
    /// Degradation preference.
    pub mode: Mode,
    /// Frame-rate steps, best first (e.g. `[60, 30, 15]`).
    pub fps_steps: Vec<u32>,
    /// Resolution scale steps, best first (e.g. `[1.0, 0.75, 0.5]`).
    pub scale_steps: Vec<f32>,
    /// Bitrate floor at full resolution and the highest fps.
    pub floor_bps_at_full: u32,
    /// Upper bitrate given to the encoder.
    pub max_bps: u32,
    /// Required headroom above a better level's floor before stepping up.
    pub upgrade_margin: f32,
    /// How long the headroom must persist before stepping up.
    pub upgrade_hold_us: u64,
}

impl AdaptConfig {
    /// Defaults for a mode: 60/30/15 fps, 1.0/0.75/0.5 scale, 2.5 Mbps floor at full
    /// quality, 3 s hold, 15 % margin.
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            fps_steps: vec![60, 30, 15],
            scale_steps: vec![1.0, 0.75, 0.5],
            floor_bps_at_full: 2_500_000,
            max_bps: 50_000_000,
            upgrade_margin: 0.15,
            upgrade_hold_us: 3_000_000,
        }
    }
}

/// One rung of the degradation ladder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Level {
    /// Frame rate.
    pub fps: u32,
    /// Resolution scale.
    pub scale: f32,
    /// Lowest bitrate this level is used at.
    pub floor_bps: u32,
}

/// Target bitrate → encoder settings with hysteresis; see module docs.
#[derive(Debug)]
pub struct QualityController {
    config: AdaptConfig,
    levels: Vec<Level>,
    index: usize,
    upgrade_since_us: Option<u64>,
    settings: EncoderSettings,
}

impl QualityController {
    /// Build the ladder for `config`. Empty step lists fall back to `[60]` / `[1.0]`.
    pub fn new(mut config: AdaptConfig) -> Self {
        if config.fps_steps.is_empty() {
            config.fps_steps = vec![60];
        }
        if config.scale_steps.is_empty() {
            config.scale_steps = vec![1.0];
        }
        let levels = build_levels(&config);
        let best = levels[0];
        Self {
            settings: EncoderSettings {
                bitrate_bps: config.max_bps,
                fps: best.fps,
                scale: best.scale,
            },
            config,
            levels,
            index: 0,
            upgrade_since_us: None,
        }
    }

    /// Controller with [`AdaptConfig::new`] defaults.
    pub fn with_mode(mode: Mode) -> Self {
        Self::new(AdaptConfig::new(mode))
    }

    /// The ladder, best level first.
    pub fn levels(&self) -> &[Level] {
        &self.levels
    }

    /// Current level.
    pub fn level(&self) -> Level {
        self.levels[self.index]
    }

    /// Settings from the last update.
    pub fn settings(&self) -> EncoderSettings {
        self.settings
    }

    /// Feed a new target bitrate at time `now_us`; returns the settings to apply.
    pub fn update(&mut self, target_bps: u32, now_us: u64) -> EncoderSettings {
        // Step down as far as needed, immediately.
        while self.index + 1 < self.levels.len() && target_bps < self.levels[self.index].floor_bps {
            self.index += 1;
            self.upgrade_since_us = None;
        }
        // Step up one level after sustained headroom.
        if self.index > 0 {
            let better = self.levels[self.index - 1];
            let needed =
                f64::from(better.floor_bps) * (1.0 + f64::from(self.config.upgrade_margin));
            if f64::from(target_bps) >= needed {
                let since = *self.upgrade_since_us.get_or_insert(now_us);
                if now_us.saturating_sub(since) >= self.config.upgrade_hold_us {
                    self.index -= 1;
                    self.upgrade_since_us = None;
                }
            } else {
                self.upgrade_since_us = None;
            }
        }
        let level = self.levels[self.index];
        self.settings = EncoderSettings {
            bitrate_bps: target_bps.min(self.config.max_bps),
            fps: level.fps,
            scale: level.scale,
        };
        self.settings
    }
}

fn build_levels(config: &AdaptConfig) -> Vec<Level> {
    let max_fps = config.fps_steps.iter().copied().max().unwrap_or(60).max(1);
    let floor = |fps: u32, scale: f32| {
        let bps = f64::from(config.floor_bps_at_full)
            * f64::from(scale)
            * f64::from(scale)
            * f64::from(fps)
            / f64::from(max_fps);
        // ≤ floor_bps_at_full since scale ≤ 1 and fps ≤ max_fps; non-negative.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let bps = bps.round().clamp(0.0, f64::from(u32::MAX)) as u32;
        bps
    };
    let best_scale = config.scale_steps[0];
    let worst_scale = config.scale_steps[config.scale_steps.len() - 1];
    let best_fps = config.fps_steps[0];
    let worst_fps = config.fps_steps[config.fps_steps.len() - 1];
    let pairs: Vec<(u32, f32)> = match config.mode {
        Mode::Quality => config
            .fps_steps
            .iter()
            .map(|&f| (f, best_scale))
            .chain(config.scale_steps.iter().skip(1).map(|&s| (worst_fps, s)))
            .collect(),
        Mode::Latency => config
            .scale_steps
            .iter()
            .map(|&s| (best_fps, s))
            .chain(config.fps_steps.iter().skip(1).map(|&f| (f, worst_scale)))
            .collect(),
    };
    pairs
        .into_iter()
        .map(|(fps, scale)| Level {
            fps,
            scale,
            floor_bps: floor(fps, scale),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;

    #[test]
    fn quality_ladder_drops_fps_then_resolution() {
        let c = QualityController::with_mode(Mode::Quality);
        let ladder: Vec<_> = c.levels().iter().map(|l| (l.fps, l.scale)).collect();
        assert_eq!(
            ladder,
            vec![(60, 1.0), (30, 1.0), (15, 1.0), (15, 0.75), (15, 0.5)]
        );
        assert!(
            c.levels()
                .windows(2)
                .all(|w| w[0].floor_bps > w[1].floor_bps)
        );
    }

    #[test]
    fn latency_ladder_drops_resolution_then_fps() {
        let c = QualityController::with_mode(Mode::Latency);
        let ladder: Vec<_> = c.levels().iter().map(|l| (l.fps, l.scale)).collect();
        assert_eq!(
            ladder,
            vec![(60, 1.0), (60, 0.75), (60, 0.5), (30, 0.5), (15, 0.5)]
        );
        assert!(
            c.levels()
                .windows(2)
                .all(|w| w[0].floor_bps > w[1].floor_bps)
        );
    }

    #[test]
    fn bitrate_drops_first_above_floor() {
        let mut c = QualityController::with_mode(Mode::Quality);
        let s = c.update(3_000_000, 0);
        assert_eq!((s.bitrate_bps, s.fps, s.scale), (3_000_000, 60, 1.0));
        let s = c.update(2_500_000, S);
        assert_eq!((s.fps, s.scale), (60, 1.0), "at the floor the level holds");
    }

    #[test]
    fn steps_down_immediately_and_far_enough() {
        let mut c = QualityController::with_mode(Mode::Quality);
        let s = c.update(1_000_000, 0); // floors: 2.5M, 1.25M, 625k
        assert_eq!((s.fps, s.scale), (15, 1.0));
        let s = c.update(100_000, 1);
        assert_eq!((s.fps, s.scale), (15, 0.5), "worst level is the bottom");
        assert_eq!(s.bitrate_bps, 100_000);

        let mut g = QualityController::with_mode(Mode::Latency);
        let s = g.update(1_000_000, 0); // floors: 2.5M, 1.406M, 625k
        assert_eq!((s.fps, s.scale), (60, 0.5));
    }

    #[test]
    fn steps_up_only_after_hold_one_level_at_a_time() {
        let mut c = QualityController::with_mode(Mode::Quality);
        c.update(500_000, 0);
        assert_eq!(c.level().fps, 15);
        assert_eq!(c.level().scale, 0.75);
        let mut t = 0;
        // Plenty of headroom, but nothing changes before the hold expires
        // (the timer starts at the first update with headroom, t = 0.1 s).
        for _ in 0..30 {
            t += S / 10;
            assert_eq!(c.update(10_000_000, t).scale, 0.75);
        }
        t += S / 10;
        assert_eq!(c.update(10_000_000, t).scale, 1.0, "one step after 3 s");
        assert_eq!(c.level().fps, 15);
        t += S;
        assert_eq!(c.update(10_000_000, t).fps, 15, "hold restarts per step");
        t += 3 * S;
        assert_eq!(c.update(10_000_000, t).fps, 30);
    }

    #[test]
    fn no_flapping_around_a_floor() {
        let mut c = QualityController::with_mode(Mode::Quality);
        let mut changes = 0;
        let mut last = c.update(1_200_000, 0);
        // Target oscillates ±10% around the 30 fps floor (1.25 Mbps) every 500 ms.
        for i in 1..100u64 {
            let target = if i % 2 == 0 { 1_350_000 } else { 1_150_000 };
            let s = c.update(target, i * S / 2);
            if s.fps != last.fps || (s.scale - last.scale).abs() > f32::EPSILON {
                changes += 1;
            }
            last = s;
        }
        assert_eq!(changes, 0, "within the margin no upgrade happens");
    }

    #[test]
    fn dip_resets_upgrade_timer() {
        let mut c = QualityController::with_mode(Mode::Quality);
        c.update(1_000_000, 0);
        assert_eq!(c.level().fps, 15);
        c.update(5_000_000, S);
        c.update(1_000_000, 3 * S); // dip below the better floor + margin
        assert_eq!(c.update(5_000_000, 4 * S + S / 2).fps, 15);
        assert_eq!(c.update(5_000_000, 7 * S + S / 2).fps, 30);
    }

    #[test]
    fn bitrate_capped_at_max() {
        let mut c = QualityController::new(AdaptConfig {
            max_bps: 8_000_000,
            ..AdaptConfig::new(Mode::Latency)
        });
        assert_eq!(c.update(20_000_000, 0).bitrate_bps, 8_000_000);
    }

    #[test]
    fn empty_steps_fall_back() {
        let c = QualityController::new(AdaptConfig {
            fps_steps: vec![],
            scale_steps: vec![],
            ..AdaptConfig::new(Mode::Quality)
        });
        assert_eq!(c.levels().len(), 1);
        assert_eq!(c.level().fps, 60);
    }
}
