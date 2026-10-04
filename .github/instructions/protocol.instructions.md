---
applyTo: 'proto/**, crates/scrin-proto/**'
---

# Wire protocol

The `.proto` files in `proto/scrin/v1/` are the contract between every client and host version in
the wild. A broken contract means an old Android build cannot control a new Windows host.

## Compatibility rules

- **Additive only** inside `scrin.v1`: new fields, new messages, new enum values, new `oneof` arms.
- **Never renumber or reuse a field tag**, never change a field's type, never rename an enum value
  that is already released.
- When removing a field: delete it and add `reserved <tag>;` **and** `reserved "<name>";`.
- Enums start with `<ENUM>_UNSPECIFIED = 0`; receivers treat unknown values as unspecified.
- Receivers ignore unknown fields and unknown `oneof` arms; never reject a message because it has
  more than you understand.
- Breaking changes require a new package (`scrin.v2`) plus an ADR in `docs/adr/` and capability
  negotiation in `hello.proto`. In practice: do not.

## Style (buf lint)

- File-per-domain: `envelope`, `hello`, `auth`, `session`, `input`, `media`, `transfer`.
- `PascalCase` messages, `snake_case` fields, `UPPER_SNAKE` enum values prefixed with the enum name.
- Every message and field has a leading comment saying what it means and its unit (ms, px, bytes).
- Sizes and counts are `uint32`/`uint64`; timestamps are `uint64` microseconds of a monotonic clock
  unless stated otherwise. Never `float` for anything that is compared for equality.
- Bound everything: each repeated/bytes field documents its max length and the decoder enforces it.

## Media hot path

- Video/audio datagrams use the fixed **16-byte header** hand-packed in `crates/scrin-proto/src/media.rs`,
  not protobuf. Shards are ≤ 1200 bytes. Byte order is big-endian, layout documented field by field.
- Changing the header layout bumps the header version nibble and needs an ADR.
- Parsing is total: any byte slice yields `Ok` or `Err`, never a panic (property-tested).

## Code generation

- Rust: `crates/scrin-proto/build.rs` with `prost-build` + `protoc-bin-vendored`. Generated code is
  not edited by hand.
- TypeScript (`packages/protocol`) and Kotlin are generated from the same `.proto` via `buf.gen.yaml`.
  Regenerate all of them in the same commit as the `.proto` change.

## Test vectors

- Every new or changed message gets a populated example encoded to bytes, stored as a shared vector
  (hex + JSON description) and asserted byte-for-byte in Rust, TypeScript and Kotlin tests.
- Media header vectors cover min, max and edge values of every field.

## Verify

```powershell
buf lint
buf breaking --against '.git#branch=main'
pwsh -NoProfile -File scripts/gates.ps1 -Only proto,rust
```

Use the `add-protocol-message` skill for the full step list.
