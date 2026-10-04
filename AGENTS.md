# AGENTS.md — scrin

Open-source (AGPL-3.0-only) remote desktop. The full agent guide is
[`.github/copilot-instructions.md`](.github/copilot-instructions.md); path-scoped rules are in
`.github/instructions/`, procedures in `.github/skills/`. This file is the short version for
harnesses that read only `AGENTS.md`.

## Read first

`docs/ARCHITECTURE.md` (design) · `docs/TRACKER.md` + `docs/tracker.csv` (decisions, status) ·
`docs/adr/` (why).

## Rules

- Crypto: the one-time code never travels (SPAKE2 bound to endpoint ids + ALPN, SAS emoji);
  never log secrets; constant-time compares; crypto changes need ADR-0004 + test vectors.
- Wire: `proto/scrin/v1` is additive only; `buf breaking` must pass.
- Rust: edition 2024, clippy pedantic `-D warnings`, no `unwrap()` outside tests, `// SAFETY:` on
  every `unsafe`, `[lints] workspace = true` everywhere.
- UI: Base UI (no Radix), `motion/react` (no framer-motion), EN + RO strings, WCAG 2.2 AA,
  compositor-only animation, no `console.log`.
- Android: attended host, never `isAccessibilityTool="true"`.
- Licence `AGPL-3.0-only` in every manifest; deps must pass `deny.toml`; no Resend, no tsup.

## Verify

```powershell
pwsh -NoProfile -File scripts/gates.ps1                 # all lanes
pwsh -NoProfile -File scripts/gates.ps1 -Only invariants
pwsh -NoProfile -File scripts/check-tracker.ps1
```

A SKIP is not a PASS. A tracker row is `done` only with evidence (command + result).

## Commit

Conventional Commits, header ≤ 100, `git commit -s` (DCO). Shared clone: stage explicit paths,
never `git add -A`.
