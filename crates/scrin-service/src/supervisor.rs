//! Which session the agent should run in, and when to (re)start it.
//!
//! Pure state machine: the Windows layer feeds it console-session changes,
//! agent exits and a clock, and executes the [`Action`]s it returns. No Win32
//! here, so every rule is unit tested.

use std::time::Duration;

/// `WTSGetActiveConsoleSessionId` when no session is attached to the console.
pub const NO_SESSION: u32 = 0xFFFF_FFFF;

/// An agent that stayed up this long was healthy: its next crash restarts fast.
const HEALTHY_RUN: Duration = Duration::from_secs(60);

/// Doubling restart delay, 1 s → 60 s.
#[derive(Debug, Clone)]
pub struct Backoff {
    next: Duration,
    min: Duration,
    max: Duration,
}

impl Backoff {
    #[must_use]
    pub fn new(min: Duration, max: Duration) -> Self {
        Self {
            next: min,
            min,
            max,
        }
    }

    /// The delay to wait now; the following one doubles (capped).
    pub fn next_delay(&mut self) -> Duration {
        let d = self.next;
        self.next = (self.next * 2).min(self.max);
        d
    }

    pub fn reset(&mut self) {
        self.next = self.min;
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new(Duration::from_secs(1), Duration::from_secs(60))
    }
}

/// What the Windows layer must do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Start the agent in this session.
    Launch(u32),
    /// Stop the agent running in this session.
    Kill(u32),
}

#[derive(Debug, Clone, Copy)]
struct Running {
    session: u32,
    started_ms: u64,
}

/// Keeps one agent in the active console session.
#[derive(Debug, Default)]
pub struct Supervisor {
    target: Option<u32>,
    running: Option<Running>,
    launch_at_ms: Option<u64>,
    backoff: Backoff,
    stopping: bool,
}

impl Supervisor {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Session the agent should be in, if any.
    #[must_use]
    pub fn target(&self) -> Option<u32> {
        self.target
    }

    /// Session the agent is running in, if any.
    #[must_use]
    pub fn running(&self) -> Option<u32> {
        self.running.map(|r| r.session)
    }

    /// The console session changed (service start, logon, fast user switch,
    /// RDP takeover). `session` is `WTSGetActiveConsoleSessionId`.
    pub fn on_console(&mut self, session: u32, now_ms: u64) -> Vec<Action> {
        if self.stopping {
            return Vec::new();
        }
        // Session 0 is the services session: nothing to show there.
        let target = (session != NO_SESSION && session != 0).then_some(session);
        let mut out = Vec::new();
        if let Some(r) = self.running
            && Some(r.session) != target
        {
            out.push(Action::Kill(r.session));
            self.running = None;
        }
        if self.target != target {
            self.backoff.reset();
            self.launch_at_ms = target.map(|_| now_ms);
        }
        self.target = target;
        out.extend(self.on_tick(now_ms));
        out
    }

    /// The agent in `session` exited (crash, user ended it, logoff).
    pub fn on_agent_exit(&mut self, session: u32, now_ms: u64) {
        let Some(r) = self.running.filter(|r| r.session == session) else {
            return;
        };
        self.running = None;
        if now_ms.saturating_sub(r.started_ms) >= duration_ms(HEALTHY_RUN) {
            self.backoff.reset();
        }
        self.schedule(now_ms);
    }

    /// Launching failed (e.g. the session is mid-logon); try again later.
    pub fn on_launch_failed(&mut self, session: u32, now_ms: u64) {
        if self.running.is_some_and(|r| r.session == session) {
            self.running = None;
        }
        self.schedule(now_ms);
    }

    pub fn on_tick(&mut self, now_ms: u64) -> Vec<Action> {
        if self.stopping || self.running.is_some() {
            return Vec::new();
        }
        match (self.target, self.launch_at_ms) {
            (Some(session), Some(at)) if now_ms >= at => {
                self.launch_at_ms = None;
                self.running = Some(Running {
                    session,
                    started_ms: now_ms,
                });
                vec![Action::Launch(session)]
            }
            _ => Vec::new(),
        }
    }

    /// Service stop: kill the agent and launch nothing more.
    pub fn stop(&mut self) -> Vec<Action> {
        self.stopping = true;
        self.launch_at_ms = None;
        self.running
            .take()
            .map(|r| vec![Action::Kill(r.session)])
            .unwrap_or_default()
    }

    fn schedule(&mut self, now_ms: u64) {
        if self.target.is_some() && !self.stopping {
            let delay = duration_ms(self.backoff.next_delay());
            self.launch_at_ms = Some(now_ms.saturating_add(delay));
        }
    }
}

fn duration_ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_caps() {
        let mut b = Backoff::new(Duration::from_secs(1), Duration::from_secs(5));
        let got: Vec<u64> = (0..5).map(|_| b.next_delay().as_secs()).collect();
        assert_eq!(got, [1, 2, 4, 5, 5]);
        b.reset();
        assert_eq!(b.next_delay().as_secs(), 1);
    }

    #[test]
    fn launches_in_the_console_session_and_ignores_session_zero() {
        let mut s = Supervisor::new();
        assert_eq!(s.on_console(0, 0), []);
        assert_eq!(s.on_console(NO_SESSION, 0), []);
        assert_eq!(s.on_console(2, 10), [Action::Launch(2)]);
        assert_eq!(s.running(), Some(2));
        // Same session again: nothing to do.
        assert_eq!(s.on_console(2, 20), []);
    }

    #[test]
    fn user_switch_kills_the_old_agent_and_starts_a_new_one() {
        let mut s = Supervisor::new();
        s.on_console(1, 0);
        assert_eq!(s.on_console(3, 5), [Action::Kill(1), Action::Launch(3)]);
        // Console detached (RDP took it): just kill.
        assert_eq!(s.on_console(NO_SESSION, 6), [Action::Kill(3)]);
        assert_eq!(s.target(), None);
    }

    #[test]
    fn crashes_restart_with_growing_delay_then_reset_after_a_healthy_run() {
        let mut s = Supervisor::new();
        s.on_console(1, 0);
        s.on_agent_exit(1, 100);
        assert_eq!(s.on_tick(1_099), []);
        assert_eq!(s.on_tick(1_100), [Action::Launch(1)]);
        s.on_agent_exit(1, 1_200);
        assert_eq!(s.on_tick(3_199), [], "second crash waits 2 s");
        assert_eq!(s.on_tick(3_200), [Action::Launch(1)]);
        // Healthy for a minute, then a crash: back to 1 s.
        s.on_agent_exit(1, 3_200 + 60_000);
        assert_eq!(s.on_tick(3_200 + 61_000), [Action::Launch(1)]);
    }

    #[test]
    fn exit_of_an_agent_we_no_longer_track_is_ignored() {
        let mut s = Supervisor::new();
        s.on_console(1, 0);
        s.on_console(2, 1);
        s.on_agent_exit(1, 2);
        assert_eq!(s.running(), Some(2));
    }

    #[test]
    fn failed_launch_is_retried() {
        let mut s = Supervisor::new();
        s.on_console(4, 0);
        s.on_launch_failed(4, 0);
        assert_eq!(s.running(), None);
        assert_eq!(s.on_tick(1_000), [Action::Launch(4)]);
    }

    #[test]
    fn stop_kills_and_never_relaunches() {
        let mut s = Supervisor::new();
        s.on_console(1, 0);
        assert_eq!(s.stop(), [Action::Kill(1)]);
        assert_eq!(s.on_console(2, 1), []);
        assert_eq!(s.on_tick(100_000), []);
    }
}
