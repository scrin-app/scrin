---
applyTo: '**/*.rs, **/Cargo.toml'
---

# Rust in scrin

## Workspace and manifests

- Edition 2024, resolver 3, MSRV = `rust-version` in the root `Cargo.toml` (1.91, forced by iroh 1.x).
  Raise it only when a dependency forces it and name that dependency in the commit body.
- Every crate: `version.workspace = true`, `edition.workspace = true`, `license.workspace = true`
  (AGPL-3.0-only), `rust-version.workspace = true`, and `[lints] workspace = true`.
- Shared dependencies go in `[workspace.dependencies]` and are referenced with `{ workspace = true }`.
  Never pin a second version of a crate in a member manifest.
- A new crate is added to `[workspace] members` in the same commit, plus a row in `docs/ARCHITECTURE.md` § 2.

## Lints and style

- `cargo fmt --all` (rustfmt edition 2024, `max_width = 100`) and
  `cargo clippy --workspace --all-targets -- -D warnings` (pedantic) must be clean.
- `unwrap()`/`expect()` only in tests and `build.rs`. Library code returns `Result` with a
  `thiserror` enum per crate; binaries (`scrin-server`, `scrin-service`) may use `anyhow` at the edge.
- Every `#[allow(...)]` / `#[expect(...)]` has a comment directly above saying why the lint's advice
  does not apply here. Prefer `#[expect]` so a stale allowance fails the build.
- No `dbg!`, no `todo!`, no `println!` in library code — use `tracing` with structured fields.
- `unsafe` only in `scrin-win`, `scrin-ffi` and other FFI edges. Each `unsafe` block carries a
  `// SAFETY:` comment; `unsafe_op_in_unsafe_fn` is denied.

## Architecture rules

- `scrin-session` is pure: `on_event` / `on_tick` return actions, no I/O, no clock reads, no
  randomness. Inject time and RNG so state-machine tests are deterministic.
- Types that cross the wire come from `scrin-proto` (prost) — never define a parallel struct.
  The media hot path uses the hand-packed 16-byte header in `scrin-proto`, not protobuf.
- Crypto lives only in `scrin-crypto`; other crates call its API. See `security.instructions.md`.
- Platform code is behind `#[cfg(windows)]` / `#[cfg(target_os = "android")]` with a portable
  branch that returns an `Unsupported` error, so `cargo check` passes on every target.
- No blocking calls on the tokio runtime (capture, encode, file I/O): use `spawn_blocking` or a
  dedicated thread publishing over a channel. Hot loops never allocate per frame — reuse buffers.
- `scrin-ffi` (UniFFI) and `scrin-wasm` (wasm-bindgen) are thin: no logic that is not tested in
  the underlying crate. Changing an exported signature means rebuilding Android bindings
  (`android/core-ffi`) and the web package in the same change.

## Tests

- Unit tests next to the code; cross-crate flows in `crates/<crate>/tests/`.
- Wire changes add a test vector shared with TS and Kotlin (see `protocol.instructions.md`).
- Property tests (`proptest`) for parsers, packetizer, FEC and framing; fuzz-friendly decoders
  never panic on hostile input — return `Err`.
- `cargo test --workspace` must pass; Windows-only tests are `#[cfg(windows)]`, not `#[ignore]`.

## Verify

```powershell
pwsh -NoProfile -File scripts/gates.ps1 -Only rust
```

The log is `.copilot-tmp/gates/<stamp>/rust.log`. A claim that code works is VERIFIED only after
that lane is green; anything running on a device needs the `device-verify` skill.
