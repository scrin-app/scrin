//! Process counters exposed at `/metrics` in the Prometheus text format.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct Metrics {
    pub registrations: AtomicU64,
    pub presence_updates: AtomicU64,
    pub resolves: AtomicU64,
    pub resolves_offline: AtomicU64,
    pub rate_limited: AtomicU64,
    pub locked_out: AtomicU64,
    pub failure_reports: AtomicU64,
    pub abuse_reports: AtomicU64,
    pub blocked_rejections: AtomicU64,
    pub bad_signatures: AtomicU64,
    pub relay_active: AtomicI64,
    pub relay_denied: AtomicU64,
    pub gateway_active: AtomicI64,
    pub gateway_sessions: AtomicU64,
    pub gateway_bytes: AtomicU64,
    pub locator_allocations: AtomicU64,
    pub locator_lookups: AtomicU64,
    pub locator_misses: AtomicU64,
}

pub fn inc(c: &AtomicU64) {
    c.fetch_add(1, Ordering::Relaxed);
}

/// One exported series: name, help, type, reader.
type Series = (
    &'static str,
    &'static str,
    &'static str,
    fn(&Metrics) -> i128,
);

const COUNTER: &str = "counter";
const GAUGE: &str = "gauge";

const SERIES: [Series; 18] = [
    (
        "scrin_registrations_total",
        "New scrin IDs allocated.",
        COUNTER,
        |m| load(&m.registrations),
    ),
    (
        "scrin_presence_updates_total",
        "Accepted presence heartbeats.",
        COUNTER,
        |m| load(&m.presence_updates),
    ),
    (
        "scrin_resolves_total",
        "Successful ID resolutions.",
        COUNTER,
        |m| load(&m.resolves),
    ),
    (
        "scrin_resolves_offline_total",
        "Resolutions of unknown or offline IDs.",
        COUNTER,
        |m| load(&m.resolves_offline),
    ),
    (
        "scrin_rate_limited_total",
        "Requests refused by a rate limit.",
        COUNTER,
        |m| load(&m.rate_limited),
    ),
    (
        "scrin_locked_out_total",
        "Resolutions refused by the pairing-failure lockout.",
        COUNTER,
        |m| load(&m.locked_out),
    ),
    (
        "scrin_failure_reports_total",
        "Failed-pairing reports from hosts.",
        COUNTER,
        |m| load(&m.failure_reports),
    ),
    (
        "scrin_abuse_reports_total",
        "Abuse reports received.",
        COUNTER,
        |m| load(&m.abuse_reports),
    ),
    (
        "scrin_blocked_rejections_total",
        "Requests refused because the key is blocked.",
        COUNTER,
        |m| load(&m.blocked_rejections),
    ),
    (
        "scrin_bad_signatures_total",
        "Requests with a bad signature or timestamp.",
        COUNTER,
        |m| load(&m.bad_signatures),
    ),
    (
        "scrin_relay_denied_total",
        "Relay connections refused by access control.",
        COUNTER,
        |m| load(&m.relay_denied),
    ),
    (
        "scrin_gateway_sessions_total",
        "Gateway sessions accepted.",
        COUNTER,
        |m| load(&m.gateway_sessions),
    ),
    (
        "scrin_gateway_bytes_total",
        "Bytes forwarded by the gateway (both directions).",
        COUNTER,
        |m| load(&m.gateway_bytes),
    ),
    (
        "scrin_locator_allocations_total",
        "Passphrase locators allocated (including rotations).",
        COUNTER,
        |m| load(&m.locator_allocations),
    ),
    (
        "scrin_locator_lookups_total",
        "Locator lookups that found a live device.",
        COUNTER,
        |m| load(&m.locator_lookups),
    ),
    (
        "scrin_locator_misses_total",
        "Locator lookups of unknown or expired locators.",
        COUNTER,
        |m| load(&m.locator_misses),
    ),
    (
        "scrin_relay_active_connections",
        "Relay client connections open now.",
        GAUGE,
        |m| i128::from(m.relay_active.load(Ordering::Relaxed)),
    ),
    (
        "scrin_gateway_active_sessions",
        "Gateway sessions open now.",
        GAUGE,
        |m| i128::from(m.gateway_active.load(Ordering::Relaxed)),
    ),
];

impl Metrics {
    /// Renders every series; `devices` is read from the store by the caller.
    #[must_use]
    pub fn render(&self, devices: u64) -> String {
        let mut out = String::new();
        let all = SERIES
            .iter()
            .map(|(n, h, k, f)| (*n, *h, *k, f(self)))
            .chain([(
                "scrin_registered_devices",
                "Devices with a scrin ID.",
                GAUGE,
                i128::from(devices),
            )]);
        for (name, help, kind, v) in all {
            let _ = writeln!(
                out,
                "# HELP {name} {help}\n# TYPE {name} {kind}\n{name} {v}"
            );
        }
        out
    }
}

fn load(c: &AtomicU64) -> i128 {
    i128::from(c.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_prometheus_text() {
        let m = Metrics::default();
        inc(&m.registrations);
        inc(&m.registrations);
        m.relay_active.fetch_add(3, Ordering::Relaxed);
        let text = m.render(7);
        assert!(
            text.contains(
                "# TYPE scrin_registrations_total counter\nscrin_registrations_total 2\n"
            )
        );
        assert!(text.contains("scrin_relay_active_connections 3\n"));
        assert!(text.contains("scrin_registered_devices 7\n"));
    }
}
