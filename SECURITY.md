# Security policy

Remote-desktop software is a high-value target. We treat every report seriously and credit
reporters who want credit.

## Reporting a vulnerability

**Do not open a public issue.** Report privately through
[GitHub Security Advisories](https://github.com/scrin-app/scrin/security/advisories/new)
("Report a vulnerability" on the Security tab). Include affected version/commit, platform,
reproduction steps and impact.

We aim to acknowledge within **3 working days**, give an initial assessment within **10**, and
ship a fix for critical issues within **30 days**. We coordinate disclosure with you and publish
an advisory (with a CVE where applicable) once a fix is available.

## Supported versions

Pre-1.0: only the latest release and `main` receive security fixes.

## Scope

In scope:

- Clients and hosts (Windows, Android, web), the Windows service/agent.
- `scrin-server` (rendezvous, relay, WebTransport gateway) and the accounts API.
- The wire protocol and cryptography (`crates/scrin-crypto`, `crates/scrin-net`, `proto/`).
- Update and release integrity (updater manifests, release artefacts, CI workflows).

Out of scope: social-engineering of users without a product flaw, denial of service by volume
against the public instance, findings that need a rooted/compromised host already, missing
hardening headers without demonstrated impact, issues in third-party dependencies already
publicly disclosed (tell us anyway if we are slow to update).

## Cryptography overview

Full model: [ADR-0004](docs/adr/0004-security-model.md).

- **Device identity**: Ed25519 key per install (= iroh endpoint id), sealed at rest (DPAPI,
  Android Keystore, non-extractable WebCrypto).
- **Transport**: QUIC with TLS 1.3 (rustls) authenticated by the device keys; relays forward
  ciphertext only. The browser gateway terminates WebTransport TLS but the session inside is a
  second handshake to the host key, so the gateway sees ciphertext only.
- **Quick connect**: SPAKE2 over the one-time code, bound to both endpoint ids and the ALPN, with
  key confirmation; a 5-emoji SAS shown on both sides. An attacker gets one online guess per code.
- **Unattended**: host-side trust list of controller keys; optional permanent password stored as
  an Argon2id verifier and checked through the PAKE, never sent.
- **Server RPC**: every rendezvous request is signed by the device key; an ID is bound to its key.
- **Audit**: append-only records chained by hash and signed with Ed25519.

Known limitations: the RustCrypto `spake2` crate has not had an independent audit; binaries are
currently **unsigned** (ADR-0011).
