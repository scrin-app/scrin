# ADR-0004: Security model — device keys, SPAKE2 codes, SAS, trust list

Date: 2026-10-04 · Status: Accepted · Tracker: D14, D05, D19

## Context

A remote-desktop tool grants full control of a machine, and it is a favourite tool of scammers.
Quick connect must work with no account (D05) using a short code read over the phone. A short
code must not be attackable offline, and neither the rendezvous server nor a relay nor the
browser gateway may be able to impersonate a peer. Prior art shows the failure modes: RustDesk
CVE-2024-25140 (a test root certificate shipped and trusted), CVE-2026-30784 (server endpoint
missing authentication), CVE-2026-30785 (weakly encrypted config storing secrets).

## Decision

- **Device identity**: Ed25519 keypair per install = iroh `EndpointId`. Secret sealed at rest
  with DPAPI (Windows), an Android Keystore wrapping key, or a non-extractable WebCrypto key in
  IndexedDB (web). Never written in plaintext config.
- **scrin ID**: 9-digit random number bound on the rendezvous server to the device public key;
  re-registration needs a signature by that key; every server RPC is signed (no unauthenticated
  mutating endpoint).
- **One-time code**: 8 chars from a 30-symbol unambiguous alphabet (~39 bits), valid 10 min,
  single use, regenerated after any failed attempt.
- **Pairing**: inside the QUIC channel both sides run **SPAKE2** on the code with identities bound
  to both `EndpointId`s and the ALPN `scrin/1`, followed by explicit key confirmation. The code
  never travels; one online guess per attempt, no offline dictionary attack.
- **SAS**: 5 emoji derived from the handshake transcript, shown on both screens for verbal check.
- **Unattended**: host-side trust list of controller device keys, added in an attended session
  or by org policy that the host verifies by signature. Optional permanent password stored only
  as an **Argon2id** verifier and checked through SPAKE2. Optional TOTP/passkey on the account.
- **Blind relay**: iroh relays forward QUIC ciphertext. The gateway terminates WebTransport TLS but
  an inner Noise/SPAKE2 handshake to the host key keeps session content opaque to it.
- **Permissions** per session (view, input, clipboard, files in/out, audio, mic, restart, terminal,
  record, privacy, block input, tunnel), granted by the host, revocable live.
- **Audit**: append-only records with `prev_hash` + Ed25519 signature, anchored to the server.
- **Updates**: Tauri updater with a minisign manifest; the minisign key is separate from any
  code-signing key (ADR-0011).

## Threat model

| Attacker | Mitigation |
|---|---|
| Offline brute force of the code | SPAKE2: transcript gives no offline oracle; single use, 10 min |
| Online guessing | one guess per handshake, code rotates on failure, rendezvous rate limits |
| Malicious rendezvous / ID hijack | ID bound to key, signed re-registration; peer keys verified in-band |
| Malicious relay or gateway (MITM) | SPAKE2 bound to both endpoint ids; inner handshake; SAS check |
| Stolen config / disk image | DPAPI/Keystore/WebCrypto sealing; Argon2id verifier only |
| Social-engineering scammer | anti-scam policy (ADR-0009), permissions, live revoke |
| Rogue insider in an org | signed policy, JIT grants, signed audit chain |
| Malicious update | minisign-verified manifest, separate key |

## Known risks

- The `spake2` crate (RustCrypto) is **not independently audited**. Wrapped behind `scrin-crypto`
  with test vectors so it can be replaced (e.g. CPace) without a protocol redesign beyond an ALPN
  bump.
- ~39-bit codes are safe only because guessing is online and rate-limited.
- Lessons from RustDesk CVEs are enforced as gate invariants: no test certificates or keys in
  release builds, every server route authenticated, no homemade config encryption.

## Consequences

- No account or server trust is needed to connect securely; the server is a directory, not a CA.
- Losing a device key means re-pairing; trust lists reference keys, not IDs.
- The browser client depends on WebCrypto non-extractable keys; clearing site data resets identity.

## Alternatives considered

- **OPAQUE (audited `opaque-ke`)** — an aPAKE suited to stored account passwords, but needs a
  registration phase; wrong shape for ephemeral codes. Kept as an option for account passwords only.
- **CPace** — modern balanced PAKE, but fewer mature Rust implementations today; fallback candidate.
- **Noise XXpsk3 with the code as PSK** — a low-entropy PSK is offline-brute-forceable from one
  recorded handshake. Rejected.
- **Plain password over TLS** — the server or anyone with a trusted cert sees the secret. Rejected.
- **Server-mediated trust** (server vouches for keys) — the server becomes a MITM point. Rejected.
