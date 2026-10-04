# scrin — repository instructions

scrin is an **AGPL-3.0-only open-source remote desktop**: ID + one-time code quick connect,
unattended access, gaming-grade streaming, teams/MSP. v1 platforms: Windows host+client, Android
host (attended) + client, browser client. Read `docs/ARCHITECTURE.md` for the design,
`docs/TRACKER.md` for decisions (D01…) and status, `docs/adr/` before changing anything
architectural.

## Non-negotiable invariants

1. **The code never travels.** Quick connect = SPAKE2 over the one-time code bound to both
   endpoint ids + ALPN `scrin/1`, then a 5-emoji SAS. Relays and the gateway see ciphertext only.
   Crypto changes need an ADR update (`docs/adr/0004-security-model.md`) and known-answer tests.
2. **Never log secrets**: codes, keys, passwords, session keys, tokens. Compare secrets in
   constant time (`subtle`); `zeroize` key material.
3. **Wire changes are additive.** `proto/scrin/v1` only gains fields; never renumber or reuse a
   tag; `buf breaking` against `main` must pass. The media hot path uses the hand-packed header in
   `scrin-proto`, not protobuf.
4. **Rust**: edition 2024, clippy pedantic `-D warnings`, `[lints] workspace = true` in every
   crate, no `unwrap()` outside tests (escape hatch `// scrin-allow-unwrap: <why>`), every
   `unsafe` has `// SAFETY:`, every `#[allow]` a justifying comment.
5. **UI**: Base UI only (never Radix), `motion/react` only (never framer-motion), compositor-only
   animation, reduced motion honoured, WCAG 2.2 AA, every string in EN **and** RO, no `console.log`.
6. **Android**: attended host; AccessibilityService with prominent disclosure; never
   `isAccessibilityTool="true"`.
7. **Licence**: `AGPL-3.0-only` in every `package.json` and `Cargo.toml`; dependencies must pass
   `deny.toml`. Email/billing via brivio — never Resend. Libraries build with tsdown — never tsup.
8. **Anti-scam** (ADR-0009): anonymous sessions are capped (60 min, no files, no unattended
   setup, no privacy mode); never add a path that installs unattended access from a quick session.

Invariants 4–8 are enforced by `scripts/check-invariants.ps1` (INV-01…INV-14), each
mutation-tested by `scripts/test-invariants.ps1`. Add a check + a mutation for every new rule.

## Where things live

```
proto/scrin/v1/          wire contract (buf)
crates/scrin-proto       prost types + media header      crates/scrin-crypto   identity, code, SPAKE2, SAS, trust
crates/scrin-net         iroh, framing, FEC, BWE         crates/scrin-media    codecs, packetizer, pacing
crates/scrin-session     pure state machine, permissions crates/scrin-win      DXGI, MF, input, WASAPI, clipboard
crates/scrin-ffi         UniFFI (Android)                crates/scrin-wasm     wasm-bindgen (browser)
crates/scrin-server      rendezvous + relay + gateway
apps/{desktop,web,api,site}   packages/{ui,i18n,config,protocol,sdk,cli,mcp}   android/   infra/   deploy/
docs/{ARCHITECTURE.md,TRACKER.md,tracker.csv,adr/}    scripts/   .github/{instructions,skills,workflows}
```

## Gates

| Task | Command |
|---|---|
| Everything (parallel lanes) | `pwsh -NoProfile -File scripts/gates.ps1` |
| Some lanes | `pwsh -NoProfile -File scripts/gates.ps1 -Only rust,js,proto,android,size,e2e,invariants,security` |
| Invariants / mutation proof | `scripts/check-invariants.ps1` · `scripts/test-invariants.ps1` |
| Tracker validity | `pwsh -NoProfile -File scripts/check-tracker.ps1` |
| Version bump (lockstep) | `pwsh -NoProfile -File scripts/version.ps1 0.2.0 -DryRun` |

Logs land in `.copilot-tmp/gates/<stamp>/<lane>.log`. A lane whose inputs or tool are missing
prints **SKIP** with the reason — a skip is not a pass; say so when reporting.
In a shared clone, run builds through `~/.copilot/hooks/run-build.ps1` (queued).

## Tracker rules

- **One canonical tracker**: `docs/TRACKER.md` (goal, decisions D01…, slices) + `docs/tracker.csv`
  (rows `id,epic,title,type,priority,status,evidence`). No other TODO lists or plan files.
- Status ∈ `todo|doing|done|blocked|dropped`. **`done` requires evidence** — the command and its
  result, a test name, or a screenshot path; say VERIFIED (ran it) vs EXPECTED (reasoned).
  `blocked`/`dropped` need the reason in `evidence`. Ids are never reused; decisions stay sequential.
- New architectural decision → next `Dnn` row in TRACKER.md + an ADR in `docs/adr/`.

## Commits

Conventional Commits (`type(scope): subject`, header ≤ 100 chars) with a DCO trailer
(`git commit -s`). Shared clone: stage explicit paths only. Hooks: `lefthook.yml`.

## Skills (`.github/skills/*/SKILL.md`)

| Task | Skill |
|---|---|
| New wire message | `add-protocol-message` |
| New session permission | `add-permission` |
| New UI / Android string | `add-locale-string` |
| Is it green? | `run-gates` |
| Prove it on real hardware (PC ↔ dragos-vivobook ↔ A51) | `device-verify` |
| Cut a release | `release` |

Instructions auto-apply by path: `rust`, `frontend`, `android`, `protocol`, `security`, `docs`.
