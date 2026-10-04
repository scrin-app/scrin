# ADR-0011: Code signing deferred until a legal entity exists

Date: 2026-10-04 · Status: Accepted · Tracker: D17

## Context

Windows installers and the SYSTEM service should be Authenticode-signed to avoid SmartScreen
warnings and some antivirus heuristics. The maintainer has no legal entity yet (no SRL/PFA) (Q02).
The current options:

- **Azure Artifact Signing** (formerly Trusted Signing) issues certificates only to organisations
  and to individuals in the US/Canada — not to a Romanian individual.
- **OV certificates** for individuals exist from a few CAs, but require hardware tokens/HSM since
  2023 and still start with zero SmartScreen reputation.
- **EV certificates** no longer bypass SmartScreen (Microsoft change, 2024); they cost more and
  need an organisation.

## Decision

- Release binaries (desktop installer, service, CLI) are **unsigned** for Authenticode until a legal
  entity exists (D17).
- **Integrity is still enforced**:
  - the Tauri updater only installs packages whose manifest verifies against the **minisign** public
    key embedded in the app (key separate from any future code-signing key, ADR-0004);
  - every GitHub Release publishes SHA-256 checksums;
  - builds produce **SLSA provenance** via GitHub artifact attestations
    (`actions/attest-build-provenance`), verifiable with `gh attestation verify`;
  - releases are built in CI from a clean tagged commit, never from a developer machine.
- Distribution channels that sign or vouch for us are pursued in parallel: **winget** manifest
  (hash-pinned) and evaluation of the **Microsoft Store** (Store signs MSIX packages itself).
- Android is unaffected: APKs/AABs are signed with our own upload/app keys as usual.
- **Revisit trigger**: the moment an SRL/PFA exists (Q02) — then obtain Azure Artifact Signing for
  the organisation and sign installer, service and CLI in the release workflow.

## Consequences

- Users see "Windows protected your PC" on first run of the installer; docs explain how to verify
  checksums/attestations and proceed. Some corporate environments will block unsigned binaries.
- Antivirus false positives are more likely for a remote-access tool without a signature; we
  submit releases to major vendors for whitelisting.
- The service runs as SYSTEM unsigned — acceptable only because updates are minisign-verified and
  installation needs admin consent.
- Done criteria for v1 do not require signing; SmartScreen-clean releases stay blocked on Q02.

## Alternatives considered

- **Self-signed certificate** — not trusted by Windows, gives SmartScreen no reputation, and
  teaching users to trust a custom root is exactly the RustDesk CVE-2024-25140 anti-pattern.
  Worse than unsigned. Rejected.
- **Buy an OV certificate as an individual** — cost plus HSM/token, slow reputation build-up, and it
  binds the brand to a personal name; wasted once an entity exists. Rejected for now.
- **Sign through a third party** (a friendly company or signing service) — the signature would
  assert someone else's identity and they would carry liability for a remote-access tool. Rejected.
