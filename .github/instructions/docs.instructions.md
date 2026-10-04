---
applyTo: '**/*.md, docs/**'
---

# Documentation

Docs stay true after every change. A doc that describes old behaviour is a bug.

## Canonical tracker

- `docs/TRACKER.md` holds goal, Done means, delivery slices, decisions (D-xx) and open questions (Q-xx).
- `docs/tracker.csv` holds the row-level backlog with columns, in order:
  `id,epic,title,type,priority,status,evidence`.
- `status` is one of `todo`, `doing`, `done`, `blocked`, `dropped`.
  - `done` requires `evidence`: the command or test that proves it **and** its result
    (e.g. `gates.ps1 -Only rust -> PASS 2026-10-04`, `VERIFIED: PC->vivobook SAS equal, shot .copilot-tmp/x.png`).
  - `blocked` and `dropped` require the reason in `evidence`.
  - Mark `EXPECTED` vs `VERIFIED` explicitly; only `VERIFIED` evidence can close a device-facing row.
- Quote any field containing a comma; one record per line (no embedded newlines).
- Ids are stable: never renumber or reuse an id; drop a row instead of deleting it.
- Validate after every edit: `pwsh -NoProfile -File scripts/check-tracker.ps1`.

## ADRs

- Path `docs/adr/NNNN-kebab-title.md`, next free 4-digit number, never renumbered.
- Sections, in order: **Context**, **Decision**, **Consequences**, **Alternatives**, plus `Status:`
  (Proposed / Accepted / Superseded by NNNN) and date in the header.
- An ADR is required for: new crate or app, new dependency in `scrin-crypto`, any change to the
  security model (0004), wire breaking changes, transport/codec choices, licensing and distribution.
- Superseding: write a new ADR and set the old one's status; never rewrite history in place.
- Record the decision in `docs/TRACKER.md` § Decisions with a D-xx id linking the ADR.

## Writing

- English, plain and specific. Prefer commands, paths and numbers over adjectives.
- Paths in backticks relative to the repo root. Commands are PowerShell 7 (`;`, `$env:X=`, `curl.exe`).
- No secrets, tokens, real one-time codes, IPs of private machines or personal data in docs.
- Keep `docs/ARCHITECTURE.md` in sync when a surface, crate, transport or pipeline step changes.
- `README.md` quick start must work from a clean clone; update it with any setup change.
- `CHANGELOG.md` follows Keep a Changelog; entries are written in the same PR as the change.
- User-facing docs (site, Starlight) exist in EN and RO.

## Licence and contributions

- The project is AGPL-3.0-only; do not paste code or text under incompatible licences.
- Contributions use DCO (`git commit -s`); `CONTRIBUTING.md` explains it — keep it accurate.
