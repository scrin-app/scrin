---
name: add-protocol-message
description: Add or change a message, field or enum in the scrin wire protocol (proto/scrin/v1/*.proto) and carry it through prost, TypeScript, Kotlin, shared test vectors and buf breaking. Use whenever a feature needs new data on the wire between host, client, server or browser — not for the 16-byte media header (that needs an ADR).
---

# Add a protocol message

The `.proto` files are a contract with every scrin build already installed. The rules are in
`.github/instructions/protocol.instructions.md`; this is the step list.

## 1. Pick the file and check compatibility

- Domain → file: `envelope` (framing/oneof), `hello` (capabilities), `auth` (pairing/SAS/trust),
  `session` (state, permissions), `input`, `media` (control plane only), `transfer` (files/clipboard).
- Find the next free tag and any `reserved` ranges first:

```powershell
rg -n 'reserved|= [0-9]+;' proto/scrin/v1/session.proto
```

- Never reuse a reserved or previously released tag. Additive only.

## 2. Edit the `.proto`

- Leading comment on the message and every field (meaning + unit + max length).
- New enum: first value `<ENUM>_UNSPECIFIED = 0`.
- New message carried on a stream: add an arm to the `oneof` in `envelope.proto` with a new tag.
- New capability that an old peer may not support: add a flag to `hello.proto` and gate sending on it.

## 3. Lint and breaking check

```powershell
buf lint
buf breaking --against '.git#branch=main'
```

Both must print nothing. A breaking-change report means you renumbered, removed without `reserved`,
or changed a type — undo that, do not work around it.

## 4. Regenerate every language

```powershell
cargo build -p scrin-proto                       # prost via build.rs
buf generate                                     # TS (packages/protocol) + Kotlin per buf.gen.yaml
```

Commit generated output that is checked in (TS/Kotlin) in the same commit as the `.proto`.

## 5. Shared test vector

- Add a populated instance (every field set, including edge values) to the shared vectors used by
  the Rust, TS and Kotlin tests (`rg -l 'test_vector|vectors' crates/scrin-proto packages/protocol android`
  to find the current location).
- Rust: encode → assert bytes equal the hex; decode the hex → assert equality.
- Same assertions in `packages/protocol` (vitest) and `android/core-ffi` (JUnit).

## 6. Wire it in

- Rust handlers in `scrin-session` (pure state machine: event → actions) and the sender in
  `scrin-net` / `scrin-engine`. Unknown or missing fields must not panic.
- If the message changes what a peer may do (permission, file, input), check the permission in
  `scrin-session` — see the `add-permission` skill.
- If exposed to Android or the browser, update `scrin-ffi` / `scrin-wasm` exports.

## 7. Gates

```powershell
pwsh -NoProfile -File scripts/gates.ps1 -Only proto,rust,js,android
```

Read `.copilot-tmp/gates/<stamp>/<lane>.log` for any failure.

## 8. Docs and tracker

- Update `docs/ARCHITECTURE.md` if a stream kind or flow changed.
- Tracker row in `docs/tracker.csv` with evidence; `pwsh -NoProfile -File scripts/check-tracker.ps1`.

## Done means

- [ ] `buf lint` and `buf breaking --against '.git#branch=main'` clean
- [ ] No reused/renumbered tag; removed fields `reserved` by tag and name
- [ ] Rust, TS and Kotlin regenerated in the same commit
- [ ] Shared vector asserted byte-for-byte in all three languages
- [ ] Handler tolerates unknown fields/arms; capability-gated if old peers lack it
- [ ] `gates.ps1 -Only proto,rust,js,android` green; tracker row updated with evidence
