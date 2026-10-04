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

## Rendezvous server

Set `EngineConfig::server` (desktop: `SCRIN_SERVER` env, else `settings.json` → `server`) to a
scrin-server base URL. The engine then (`src/rendezvous.rs`):

| When | Call | Signed with | Label / signed body |
|---|---|---|---|
| start | `GET /v1/info` | — | relay URLs → `RelayConfig::Custom` (falls back to the server URL itself) |
| start, then on `not_registered` | `POST /v1/register` | device key | `scrin rendezvous register v1` / address hint |
| every 30 s (≤ TTL/2), backoff 1 s → 60 s on failure | `POST /v1/presence` | device key | `scrin rendezvous presence v1` / address hint |
| connect by 9-digit ID | `POST /v1/resolve` | controller key | `scrin rendezvous resolve v1` / the id |
| wrong code at the host | `POST /v1/report-failure` | host key | `scrin rendezvous report-failure v1` / controller key hex |

Signed bytes: `label ‖ 0x00 ‖ device_pub(32) ‖ ts u64 BE ‖ len u32 BE ‖ body`. The address
hint's canonical body is `relay_url` (or empty) then one direct address per line, joined by
`\n`. `advertise_direct = false` publishes the relay only. The registered ID is persisted in
`<data_dir>/scrin-id.json` (keyed by server + device) and replaces the provisional ID in
`Status`; `Status.online` is the outcome of the last presence call. HTTPS uses rustls + ring +
webpki-roots via `tls_backend_preconfigured` (no platform verifier, which breaks on Android).

## Gateway sessions

Browsers reach a host through the server's gateway, which dials the host over iroh with ALPN
`scrin-gw/1`. The contract is **`docs/protocol/gateway-session.md`** (inner layer, authoritative)
and `crates/scrin-server/GATEWAY.md` (outer layer). The engine accepts `scrin/1` and
`scrin-gw/1` on one endpoint (`EngineConfig::accept_gateway`, default on); host half in
`src/gw.rs`.

- The iroh peer is the **gateway**, untrusted transport: its endpoint id is ignored and never
  looked up in the trust store. Gateway sessions are always `anonymous` (ADR-0009 caps).
- Every bi-stream starts with a 3-byte plaintext header `kind u8 ‖ ordinal u16 BE`;
  lane = `opener_is_host << 24 | kind << 16 | ordinal` (Control `00 00 00` → lane 0, Input
  `01 00 00` → lane `0x00010000`). Duplicate lanes / unknown kinds are reset with `0x5c01`.
- Handshake on Control (frames `u32 BE len ‖ tag ‖ payload`): `Hello(1,1,0)` both ways,
  `Identify` (tag 7, 32-byte device id) both ways, `PairStart`, `PairConfirm`, `Attest` (tag 8,
  Ed25519 over `"scrin/1" ‖ "/gateway attest v1" ‖ 'C'|'H' ‖ H id ‖ C id ‖ tagC ‖ tagH`) both
  ways, then controller `Result(0)`. SPAKE2 binds both `Identify` keys; the host shows the
  controller key only after its `Attest` verifies. `intent = 1` → `Result(WrongMode)`.
- The one-time code is shared with the native path (one guess per code); a wrong code or bad
  signature is counted locally and reported via `/v1/report-failure`.
- Channel: `Paired::export("scrin gateway channel v1")` → `scrin_crypto::channel`, side Host.
  Stream frames are sealed on their lane (ordered); every datagram on lane `0xFFFFFFFF`
  (unordered, 64-wide window). A stream frame that fails to open closes the session with
  application code `0x01`.
- Close codes (< `0x100`, passed through by the gateway): `0x00` normal, `0x01` protocol,
  `0x02` pairing failed, `0x04` handshake timeout; `0x10` host busy (not in the contract table yet).
- Proven end to end in `tests/server.rs` with the browser's own core (`scrin_wasm::core`)
  over the real WebSocket gateway.

## Reconnect

- **Trusted (unattended) sessions** survive a dropped connection: the controller emits
  `StateChanged { Connecting, reason: "reconnecting" }`, re-dials with `scrin_net::reconnect::Backoff`
  (0.5 s → 8 s, 8 attempts), re-authenticates by signature and re-sends `SessionRequest`; the
  host replaces the stale session of the same trusted key and auto-accepts (policy). On
  success: `StateChanged { Active, reason: "reconnected" }`.
- **Code sessions** cannot resume (the code is spent): `Error { code: "connection-lost" }` then
  `Ended`; the UI offers to reconnect with a new code.

## Pieces

- `secret`: `SecretStore` — `DpapiStore` (Windows, `CryptProtectData` + app entropy) and `FileStore`
  (tests/dev). Holds the 32-byte identity seed.
- `resolve`: `Resolver` — `StaticResolver` (tests), `HttpResolver` (anonymous
  `GET {server}/v1/resolve/{id}`, kept for tools) and `rendezvous::RendezvousClient` (signed
  `POST /v1/resolve`, used whenever `EngineConfig::server` is set); `scrin:` tickets.
- `rendezvous`: signed register / presence / resolve / report-failure client.
- `gw`: gateway handshake (`scrin-gw/1`) and the inner sealed channel.
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

`tests/server.rs` starts a real in-process `scrin_server::Server` (rendezvous + relay + gateway
on 127.0.0.1:0): register → signed resolve → pair over the server's relay (direct addresses
not advertised) → frames; wrong code → `scrin_failure_reports_total` + 1; and a Rust "browser"
(`scrin_wasm::core`) over the WebSocket gateway that runs the contract handshake (Identify,
SPAKE2, Attest), opens the sealed `SessionAccept`/`VideoConfig`, reassembles sealed video
datagrams and sends sealed input on the Input stream.

## Live test CLI

`examples/controller_cli.rs` (`--features win` for real capture/decoding):
`controller_cli host` prints ID + code and accepts the first request after its delay;
`controller_cli connect <id> <code> --secs 10 --png out.png` saves a decoded frame and prints
`STATS` lines (fps, RTT, bitrate, loss, decode ms). Both honour `SCRIN_SERVER` and `SCRIN_DATA`.
