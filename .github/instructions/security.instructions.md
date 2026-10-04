---
applyTo: 'crates/scrin-crypto/**, crates/scrin-net/**, crates/scrin-server/**, apps/api/src/auth/**'
---

# Security-critical code

scrin gives a stranger control of a computer. Every rule here exists because breaking it hands an
attacker a session. The model is in `docs/adr/0004-security-model.md`; read it before editing.

## Primitives

- **No custom cryptographic primitives or constructions.** Use the workspace crates:
  `ed25519-dalek` (device keys), `spake2` (code pairing), `argon2` (Argon2id password verifier),
  `blake3` (transcripts, hash chain), `subtle` (comparisons), `zeroize` (secret memory), iroh/rustls
  for transport. A new crypto dependency needs an ADR.
- Any change under `crates/scrin-crypto/` requires an update to (or a new) ADR referencing 0004 and
  **known-answer test vectors** (fixed inputs → fixed outputs), not only round-trip tests.

## Protocol invariants

- SPAKE2 identities are bound to **both iroh endpoint ids and the ALPN `scrin/1`**. Removing either
  binding enables relay/MITM attacks.
- The one-time code never travels on the wire, is single use, expires in 10 minutes and is rate-limited
  per host; failures are counted and lock out.
- The SAS is 5 emoji derived from the full transcript; both sides display it before control starts.
- Unattended access requires the controller key on the host trust list, or an org policy whose
  signature the host verifies itself. The server is never trusted to vouch for a key.
- Permanent passwords: store only the Argon2id verifier; verify through SPAKE2, never by sending it.
- Every rendezvous RPC is signed by the device key; re-registering an ID requires that key.
- The relay and the WebTransport gateway are **blind**: they forward ciphertext and must not hold
  session keys. Do not add plaintext inspection "for debugging".
- Audit records: append-only, `prev_hash` + Ed25519 signature; never update or delete a record.

## Implementation rules

- Compare MACs, tags, codes, verifiers and tokens with `subtle::ConstantTimeEq`, never `==`.
- Secret material lives in `Zeroizing<…>` or a `#[derive(Zeroize, ZeroizeOnDrop)]` type; no `Clone`
  or `Debug` that prints it (implement `Debug` manually with `"[redacted]"`).
- **Never log** secrets, one-time codes, SAS, passwords, private keys, session keys, tokens or
  clipboard/file contents — not even at `trace`. Log ids and outcomes only.
- Randomness from the OS CSPRNG (`rand::rngs::OsRng` / `getrandom`); never seeded RNGs outside tests.
- Parse untrusted input with explicit length limits before allocating; decoders return `Err`, never panic.
- Server endpoints: rate-limit by IP and by ID, reject oversized frames, time out idle handshakes.
- Anonymous sessions stay capped: no file transfer, no unattended setup, no privacy mode, 60 minutes.
- `apps/api/src/auth/**`: better-auth only; sessions in httpOnly secure cookies; authz check on every
  mutation; never return whether an email exists.

## Verify

```powershell
pwsh -NoProfile -File scripts/gates.ps1 -Only rust,security,invariants
```

Report vulnerabilities through GitHub Security Advisories (`SECURITY.md`), never a public issue.
