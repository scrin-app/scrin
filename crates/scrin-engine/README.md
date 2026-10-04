# scrin-engine

Native host + controller engine for the desktop app. One actor (tokio task) owns identity, trust
store, one-time code, the `scrin-session` state machines and all connections; the UI drives it with
`Command`s and receives `Event`s.

## API

```rust
let (engine, mut events) = scrin_engine::start(EngineConfig::new(data_dir)).await?;
engine.call(Command::GetStatus).await?;            // Reply::Status
engine.call(Command::Connect { target, code, requested: None }).await?; // Reply::Session("c1")
while let Some(ev) = events.recv().await { /* Event::… */ }
```

| Command | Effect |
|---|---|
| `GetStatus` / `RegenerateCode` | device id, provisional scrin ID, ticket, code + expiry |
| `Connect { target, code, requested }` | target = 9-digit ID (via `Resolver`), `scrin:` ticket or 64-hex id; empty code = trusted |
| `ConfirmSas { session, matches }` | `false` ends the session (`sas-mismatch`) |
| `Accept { session, permissions }` / `Reject` | host answers the interstitial (anti-scam delay enforced) |
| `Grant` / `Revoke { session, permission }` | live permission change |
| `TrustPeer { session, label }` | add controller to the sealed trust store (policy-gated) |
| `EndSession`, `SendInput`, `SetQuality` | — |
| `ListTrusted` / `RemoveTrusted { device }` | — |

Events: `Status`, `IncomingRequest`, `Sas`, `StateChanged`, `PermissionsChanged`,
`PermissionRequested`, `Stats`, `VideoFrame` (native renderer only, not serialised), `Error { code }`.

## Pieces

- `secret`: `SecretStore` — `DpapiStore` (Windows, `CryptProtectData` + app entropy) and `FileStore`
  (tests/dev). Holds the 32-byte identity seed.
- `resolve`: `Resolver` — `StaticResolver` (tests) and `HttpResolver` (`GET {server}/v1/resolve/{id}`;
  lenient until `crates/scrin-server/GATEWAY.md` fixes the contract); `scrin:` tickets.
- `backend`: `MediaBackend` (capture/encode/decode/inject) — `SyntheticBackend` (test pattern + fake
  codec), `NullBackend`, and `win_backend::WinBackend` over `scrin-win` (feature `win`).
- `media`: host thread capture → encode → `FrameEncoder` shards → QUIC datagrams; controller
  datagrams → `FrameReassembler` → decoder thread. `BitrateFeedback` every 50 ms → GCC BWE →
  quality ladder + FEC ratio.

## Datagram header

Video datagrams carry the **scrin-media `ShardHeader`** (16 bytes, includes `payload_len`, which
Reed-Solomon reassembly needs; the frame length travels inside the protected data). The
`scrin-proto::media` header is the same size without `payload_len` and is not put on the wire by
this engine. Send timestamps for BWE stay on the host, looked up by `frame_id`/`shard_index`.

## Tests

`cargo test -p scrin-engine` — unit tests plus `tests/loopback.rs`: two engines over loopback
(relays off) with the synthetic backend pair by scrin ID + code, compare SAS, accept, stream ≥ 30
frames in ≤ 3 s, inject input, revoke input, end cleanly; wrong code is refused and burns the code.
