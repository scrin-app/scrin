---
name: release
description: Cut a scrin release vX.Y.Z — bump every version with scripts/version.ps1, update CHANGELOG and every surface naming the version, tag, publish the GitHub release with checksums (unsigned binaries per ADR-0011), the Tauri updater minisign manifest, Android gms/foss builds for Play, GitHub and F-Droid, and deploy scrin-server from a clean tree. Use when the user asks to release, ship, tag or publish a version.
---

# Release scrin

Order is fixed: gates → version → changelog → commit → push → tag → build from a clean tree →
publish → deploy → verify live. Never build release artefacts from the shared clone.

## 1. Preconditions

```powershell
pwsh -NoProfile -File scripts/gates.ps1                       # all lanes, via run-build queue in a shared clone
pwsh -NoProfile -File scripts/check-tracker.ps1
git fetch origin; git status --short --branch
```

All lanes green, tracker valid, branch up to date with `origin/main`.

## 2. Find every place the old version lives

```powershell
$old = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
rg -n -F $old --glob '!target' --glob '!**/node_modules/**' --glob '!pnpm-lock.yaml' --glob '!Cargo.lock'
```

Keep this list; every hit that names the *current* release must move.

## 3. Bump in lockstep

```powershell
pwsh -NoProfile -File scripts/version.ps1 0.2.0
```

Updates the Cargo workspace version, every `package.json`, Tauri config, and Android
`versionName` / `versionCode` (`MAJOR*1_000_000 + MINOR*1_000 + PATCH` → 0.2.0 = 2000).
Re-run the `rg` from step 2: remaining hits are surfaces version.ps1 does not own (README, site,
docs, store text) — edit them now.

## 4. CHANGELOG and notes

- `CHANGELOG.md` (Keep a Changelog): move `Unreleased` to `## [0.2.0] - <date>`; Added/Changed/
  Fixed/Security; link tracker ids.
- Store "what's new" (Play per track, F-Droid `fastlane/metadata/android/{en-US,ro}/changelogs/<versionCode>.txt`)
  in EN and RO.

## 5. Commit, push, tag

```powershell
pwsh -NoProfile -File "$env:USERPROFILE\.copilot\bin\agentq.ps1" commit -Purpose 'release 0.2.0' -Message 'chore(release): v0.2.0' -Paths 'Cargo.toml,Cargo.lock,CHANGELOG.md,...'
git tag -s v0.2.0 -m 'scrin v0.2.0'; git push origin v0.2.0
```

Commits carry a DCO `Signed-off-by`; header ≤ 100 chars. Stage explicit paths only.

## 6. Build artefacts from a clean tree

```powershell
pwsh -NoProfile -File "$env:USERPROFILE\.copilot\hooks\deploy-clean.ps1" -Ref v0.2.0 -Command 'pwsh -NoProfile -File scripts/release-build.ps1'
```

(use the repo's release build script; check `rg --files scripts | rg release`). Outputs:

- Windows: NSIS/MSI installers + portable exe — **unsigned** until a signing entity exists
  (ADR-0011, D17); release notes state the SmartScreen warning.
- Tauri updater: `latest.json` signed with the **minisign** updater key (separate from code
  signing; key never in the repo or logs).
- Android: `gms` AAB for Play, `gms` + `foss` APKs for GitHub; `foss` must contain no GMS classes.
- `SHA256SUMS.txt` over every artefact: `Get-FileHash -Algorithm SHA256`.

## 7. Publish

```powershell
gh release create v0.2.0 --title 'scrin v0.2.0' --notes-file .copilot-tmp/release-notes.md <artefacts> SHA256SUMS.txt
```

- Upload the Tauri `latest.json` where the updater endpoint reads it; check it parses and the
  signature verifies.
- Play Console: upload AAB to internal → closed → production with EN+RO notes.
- F-Droid: the tag triggers the foss build from metadata; confirm `versionCode` matches.

## 8. Deploy scrin-server

Only from a clean tree, migrations first:

```powershell
pwsh -NoProfile -File "$env:USERPROFILE\.copilot\hooks\deploy-clean.ps1" -Ref v0.2.0 -Command '<server deploy script>'
```

Verify live: a real pairing through the new relay/gateway (not only `/health`), and read the
serving version/revision from that request's log line.

## 9. Close out

- Install the published Windows build on vivobook and the APK on the A51 (`device-verify` skill);
  check the updater offers the new version from the previous one.
- Tracker: release row `done` with evidence (tag, release URL, live check).

## Done means

- [ ] Gates green, tracker valid, `rg` for the old version shows no stale release references
- [ ] `version.ps1` applied; versionCode matches the formula
- [ ] CHANGELOG + store notes (EN, RO) written
- [ ] Signed-off commit pushed, tag `vX.Y.Z` pushed
- [ ] Artefacts built from a clean tree; SHA256SUMS attached; updater manifest minisign-verified
- [ ] Android gms (Play) and foss (GitHub, F-Droid) published
- [ ] scrin-server deployed from a clean tree and verified with a real session
