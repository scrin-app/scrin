## Summary

<!-- What changes and why, in a few lines. Link the ADR if this is an architectural decision. -->

## Tracker

<!-- Ids from docs/tracker.csv this PR advances or closes, e.g. S1-004, X-012. -->

-

## How it was verified

<!-- Commands run and their result. Say VERIFIED (ran it, saw it) or EXPECTED (reasoned). -->
<!-- Device-facing changes: which devices (PC, dragos-vivobook, A51, browser) and what you saw. -->

## Checklist

- [ ] `pwsh -NoProfile -File scripts/gates.ps1` green for the affected lanes (logs checked)
- [ ] Tests added or updated for the new behaviour
- [ ] User-visible strings added in **EN and RO** (`packages/i18n`, Android `values/` + `values-ro/`)
- [ ] Wire changes are additive; `buf breaking` clean; test vectors updated (Rust, TS, Kotlin)
- [ ] ADR added/updated in `docs/adr/` if architectural or security-relevant
- [ ] `docs/tracker.csv` rows updated with evidence; `scripts/check-tracker.ps1` passes
- [ ] Docs updated (`README.md`, `docs/ARCHITECTURE.md`, `CHANGELOG.md`) where behaviour changed
- [ ] Every commit is signed off (DCO, `git commit -s`) and follows Conventional Commits
- [ ] No secrets, codes, keys or personal data in code, logs or screenshots

## Screenshots

<!-- Required for UI changes: light + dark, EN + RO, narrow (360 px) and wide. -->
