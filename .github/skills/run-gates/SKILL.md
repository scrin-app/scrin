---
name: run-gates
description: Run scrin's quality gates (scripts/gates.ps1 lanes rust, js, proto, android, size, e2e, invariants, security), read the per-lane logs, fix failures at the root, and run invariants mutation tests and the tracker check. Use before every commit and push, before marking a tracker row done, and whenever someone asks "is it green".
---

# Run the gates

`scripts/gates.ps1` is the single definition of "green". CI runs the same script; never claim a
lane passes without its log.

## 1. Pick lanes for the change

| Changed | Lanes |
|---|---|
| `crates/**`, `Cargo.*`, `apps/desktop/src-tauri/**` | `rust` (+ `security` for crypto/net/server) |
| `proto/**` | `proto,rust,js,android` |
| `apps/web/**`, `apps/desktop/src/**`, `packages/**` | `js,size,e2e` |
| `android/**` | `android,size` |
| `scripts/**`, repo rules | `invariants` |
| release / before push | all (no `-Only`) |

## 2. Run

Builds are serialised per repo — run through the queue so another agent's build is not corrupted:

```powershell
pwsh -NoProfile -File "$env:USERPROFILE\.copilot\hooks\run-build.ps1" -Purpose 'gates rust' -Wait -TimeoutMin 120 -Command 'pwsh -NoProfile -File scripts/gates.ps1 -Only rust'
```

Without the queue (CI, or a clone nobody else uses):

```powershell
pwsh -NoProfile -File scripts/gates.ps1 -Only rust,js
```

## 3. Read the logs

- Per lane: `.copilot-tmp/gates/<stamp>/<lane>.log` (newest stamp = your run).

```powershell
$d = Get-ChildItem .copilot-tmp/gates -Directory | Sort-Object Name | Select-Object -Last 1
Get-ChildItem $d.FullName
rg -n 'error|FAIL|warning:' (Join-Path $d.FullName 'rust.log')
```

- Confirm the lane actually ran (look for the tool output, not only queue lines) before calling it
  failed or passed.

## 4. Fix at the root

- `rust`: `cargo fmt --all`; fix clippy findings — no blanket `#[allow]`; an allowance needs a
  justification comment. `cargo-deny` failures: licence or advisory — update or replace the crate.
  Clippy runs for three targets: Windows host, Android (`scripts/clippy-android.ps1`, NDK) and
  Linux (`scripts/clippy-linux.ps1`, WSL `Ubuntu-24.04` with `libwebkit2gtk-4.1-dev`). Code under
  `#[cfg(not(windows))]` / `#[cfg(windows)]` is only checked by the matching target: when you gate
  an item to one OS, gate its imports too. A missing WSL/NDK prints SKIP — not a pass.
- `js`: oxlint/eslint/tsc errors are fixed, not suppressed; vitest failures reproduced with
  `pnpm vitest run <file>`.
- `proto`: `buf breaking` failures are real compatibility breaks — revert the break.
- `size`: a budget overrun needs a measured reason; do not raise the budget silently.
- `e2e`: axe violations are bugs; flaky tests are fixed or quarantined with a tracker row.
- `android`: ktlint/lint/unit/assemble per flavour (`gms`, `foss`).
- One fix at a time; re-run the failing lane after each.

## 5. Invariants and their mutation tests

```powershell
pwsh -NoProfile -File scripts/check-invariants.ps1 -Root .
pwsh -NoProfile -File scripts/test-invariants.ps1
```

When you add an invariant, add a mutation case proving it fails on a violating tree.

## 6. Tracker

```powershell
pwsh -NoProfile -File scripts/check-tracker.ps1
```

Evidence for a done row: `gates.ps1 -Only <lanes> -> PASS <date>` plus the log path.

## Done means

- [ ] Every lane relevant to the change ran and its log shows success
- [ ] No new `#[allow]`, `eslint-disable`, `@ts-expect-error` without a written reason
- [ ] `check-invariants.ps1` and `test-invariants.ps1` pass when rules or scripts changed
- [ ] `check-tracker.ps1` passes; row evidence names the command and result
- [ ] Reported as VERIFIED (ran, saw the log) — never "should pass"
