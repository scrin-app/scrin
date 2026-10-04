# Contributing to scrin

Thanks for helping. scrin is AGPL-3.0-only; by contributing you agree your work is released
under the same licence.

## Developer Certificate of Origin (required)

Every commit must carry a `Signed-off-by:` trailer certifying the
[Developer Certificate of Origin 1.1](https://developercertificate.org/):

```powershell
git commit -s -m "feat(net): reconnect with backoff"
```

The `commit-msg` hook and CI reject commits without it. There is no CLA — you keep your copyright.
Use your real name or a stable pseudonym; anonymous sign-offs are not accepted.

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org/): `type(scope)?: subject`, header
≤ 100 characters, imperative, lower case.

Types: `feat` `fix` `perf` `refactor` `docs` `test` `build` `ci` `chore` `revert` `style`
`security`. Scopes are crate/app names without the prefix (`crypto`, `net`, `web`, `android`,
`server`, `ui`, …). Breaking changes: `feat(proto)!: …` plus a `BREAKING CHANGE:` footer.

## Branch flow

- `main` is protected and always releasable. Work happens on short-lived branches
  (`feat/…`, `fix/…`) merged by squash after review and a green `gate` check.
- Keep pull requests small and focused: one behaviour change, with its tests and docs.
- Rebase on `main` rather than merging it into your branch.

## Quality gates

Run locally before pushing — CI runs the same lanes:

```powershell
pwsh -NoProfile -File scripts/gates.ps1                       # all lanes in parallel
pwsh -NoProfile -File scripts/gates.ps1 -Only rust,invariants # a subset
pwsh -NoProfile -File scripts/test-invariants.ps1             # prove each invariant can fail
pwsh -NoProfile -File scripts/check-tracker.ps1               # tracker rows are valid
```

Hooks (`lefthook`): pre-commit checks secrets, formatting, lint and forbidden files on staged
files; `commit-msg` checks the header and DCO; pre-push runs `gates.ps1 -Only rust,js,invariants`.

Bar: zero warnings (`clippy -D warnings`, pedantic), no `unwrap()` outside tests, tests named
after the behaviour they prove, every user-visible string in both EN and RO.

## Architectural changes

Anything that changes the wire format, crypto, transport, storage or a public API needs an ADR in
`docs/adr/` (copy the shape of an existing one) and a row in `docs/tracker.csv`. Wire changes go
through `proto/` and must pass `buf breaking`.

## Reporting bugs and security issues

Bugs: GitHub issues using the templates. Security: **never** a public issue — see
[`SECURITY.md`](SECURITY.md).

## Code of conduct

Participation is governed by [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).
