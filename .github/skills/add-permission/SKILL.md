---
name: add-permission
description: Add a new per-session permission (alongside view, input, clipboard, files in/out, audio, mic, restart, terminal, record, privacy, block input, tunnel) end to end — scrin-session model, wire enum, enforcement on the host, live revocation, anonymous caps, UI toggles in EN+RO on desktop/web/Android, audit event and tests. Use when a feature lets the controller do something new on the host.
---

# Add a session permission

Permissions are granted by the host, revocable live, and enforced on the **host** side. A
permission that is only hidden in the controller UI is not a permission.

## 1. Find the existing model

```powershell
rg -n 'enum Permission|Permission::' crates/scrin-session/src
rg -n 'PERMISSION_' proto/scrin/v1/session.proto
```

Copy the shape of the closest sibling (e.g. `clipboard` for a data channel, `restart` for an action).

## 2. Wire enum (protocol)

- Add `PERMISSION_<NAME> = <next tag>;` to the permission enum in `proto/scrin/v1/session.proto`
  with a comment stating what it allows. Follow the `add-protocol-message` skill steps 3–5
  (`buf lint`, `buf breaking`, regenerate, vectors).

## 3. scrin-session model and enforcement

- Add the variant to the Rust `Permission` type and its mapping to/from the wire enum.
- Default: **not granted**. Decide the anonymous-session rule: anonymous sessions are capped
  (no files, no unattended setup, no privacy mode, 60 min). If the new permission is risky
  (data out, persistence, hiding the screen), add it to the anonymous deny-list.
- Enforce in the state machine: every event that uses the capability checks the grant and returns
  a `Denied` action otherwise. Revocation must stop an in-flight use (close the stream, cancel the
  transfer) on the next `on_event`/`on_tick`.
- Emit an audit event on grant, revoke and denied use (ids only, never content).

## 4. Tests (pure, in `crates/scrin-session/tests/`)

- granted → action allowed; not granted → denied; revoked mid-use → stopped;
  anonymous session → denied even if requested; wire round-trip of the enum value.

```powershell
cargo test -p scrin-session
```

## 5. Host platforms

- Windows (`scrin-engine` / `scrin-win`): gate the OS call on the session grant.
- Android host (`android/`): gate in Kotlin through the FFI grant query; if Android cannot support
  it, report it as unsupported in `hello` capabilities so the controller hides the toggle.

## 6. UI on every surface

- Host consent dialog and live permission panel: toggle with label + description.
- Controller: show the state; disable the feature when not granted.
- Strings: `packages/i18n` EN **and** RO keys (`add-locale-string` skill), Android `values/` and
  `values-ro/` strings.
- Accessible: real switch with label, state announced via `aria-live` on change.

## 7. Gates and docs

```powershell
pwsh -NoProfile -File scripts/gates.ps1 -Only proto,rust,js,android,e2e,invariants
```

- Update the permission list in `docs/ARCHITECTURE.md` § 3 and, if it changes the threat model,
  `docs/adr/0004-security-model.md` (or a new ADR).
- Tracker row with evidence; `pwsh -NoProfile -File scripts/check-tracker.ps1`.

## Done means

- [ ] Wire enum added, `buf breaking` clean, vectors updated
- [ ] Default denied; anonymous rule decided and tested
- [ ] Enforcement and live revocation on the host, with tests
- [ ] Audit events for grant/revoke/deny
- [ ] Toggles on desktop, web and Android with EN+RO strings
- [ ] ARCHITECTURE (and ADR if threat model changed) updated; gates green
