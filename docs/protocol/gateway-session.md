# Gateway session protocol (`scrin-gw/1`, inner layer)

Status: **v1 contract** for the browser client (`packages/ui/src/platform/web`, `crates/scrin-wasm`)
and the host-side `scrin-gw/1` handler (to be implemented in `crates/scrin-engine`).
Shared test vectors: [`testvectors/gateway-session.json`](../../testvectors/gateway-session.json)
(generated and checked by `cargo test -p scrin-wasm --test vectors`; replayed by the browser side in
`packages/protocol/src/wasm.test.ts`).

The outer layer — how the gateway bridges WebTransport/WebSocket to iroh — is
[`crates/scrin-server/GATEWAY.md`](../../crates/scrin-server/GATEWAY.md). The gateway is a dumb
pipe: everything below is end to end between the browser (controller) and the host. This document
only defines what differs from the native path (`scrin_net::handshake`, `scrin_net::framing`,
`crates/scrin-engine/src/{wire,media}.rs`); everything not mentioned is identical.

## 1. Why it differs from the native path

On the native path QUIC/TLS authenticates both endpoints by their device keys, so
`remote_device_id(conn)` is trustworthy and frames travel inside TLS. Through the gateway:

- the host's QUIC peer is the **gateway's** random endpoint, not the controller;
- the browser's TLS peer is the **gateway**, not the host.

So (a) both device ids travel in the handshake (`Identify`) and are bound by SPAKE2 and by a
signature (`Attest`), and (b) every byte after pairing is sealed with the inner channel
(`scrin_crypto::channel`).

## 2. Streams

Every bidirectional stream starts with a **3-byte plaintext stream header** written by the opener:

| offset | size | field |
|---|---|---|
| 0 | 1 | stream kind (`scrin_net::framing::StreamKind`: 0 Control, 1 Input, 2 Clipboard, 3 File, 4 Chat, 5 Tunnel) |
| 1 | 2 | ordinal, u16 big endian: 0 for the opener's first stream of that kind, then 1, 2, … |

After the header the stream carries frames: `u32 BE length || body` (same as
`scrin_net::framing`, max 4 MiB, reader checks the length before allocating).

The **lane** of a stream for the inner channel is

```
lane = (opener_is_host ? 1 : 0) << 24 | kind << 16 | ordinal
```

A receiver that sees a lane it has already used (duplicate header) or an unknown kind resets the
stream with code `0x5c01` (`UNKNOWN_KIND_CODE`). Uni streams are not used in v1.

The **Control** stream is opened by the controller first: header `00 00 00`, lane `0`.
The **Input** stream is opened by the controller after pairing: header `01 00 00`, lane `0x00010000`.

## 3. Handshake (Control stream, plaintext frames)

Message format = `scrin_net::handshake` (`tag u8 || payload`), with two new tags:

| tag | message | payload |
|---|---|---|
| 1 | `Hello` | `min u16 BE`, `max u16 BE`, `intent u8` (0 pair, 1 trusted) |
| 2 | `PairStart` | SPAKE2 message (33 bytes) |
| 3 | `PairConfirm` | 32-byte confirmation tag |
| 4 | `Result` | `0` ok, else a `RejectReason` byte |
| 7 | `Identify` | 32-byte Ed25519 device id of the sender |
| 8 | `Attest` | 64-byte Ed25519 signature (see below) |

Pairing (quick connect, `intent = 0`), C = browser, H = host:

```
C→H Hello(1,1,0)          H→C Hello(1,1,0)
C→H Identify(C id)        H→C Identify(H id)
C→H PairStart(mC)         H→C PairStart(mH)      ← host consumes the one-time code on mC
C→H PairConfirm(tagC)     H→C PairConfirm(tagH)  (or Result(reason))
C→H Attest(sigC)          H→C Attest(sigH)       (or Result(reason))
C→H Result(0)
```

- SPAKE2 is `scrin_crypto::pake` with `me`/`peer` taken from the `Identify` messages: host runs
  `Pairing::start(code, Role::Host, H, C)`, controller `Pairing::start(_with_entropy)(code,
  Role::Controller, C, H)`. A gateway substituting either id makes the PAKE fail (one online guess,
  then the code is gone), exactly as on the native path.
- The host sends `Result(VersionMismatch | WrongMode | CodeUnavailable | WrongCode | BadSignature)`
  instead of the next message when a step fails, and must then report the failure
  (`/v1/report-failure`) like any failed pairing.
- `Attest` proves possession of the key behind `Identify`. Each side signs

  ```
  "scrin/1" || "/gateway attest v1" || signer ('C' = 0x43 | 'H' = 0x48)
            || H id || C id || tagC || tagH
  ```

  and verifies the peer's signature with the peer's `Identify` key. The host stores the
  controller's id (trust list, audit) only after `Attest` verifies. The browser additionally
  compares `H id` with `device_pub` from `GET /v1/resolve/{id}` when that request succeeded.
- `intent = 1` (trusted, unattended) is **not** offered over the gateway in v1: every gateway
  session is anonymous (ADR-0009, GATEWAY.md §5). Hosts answer it with `Result(WrongMode)`.
- Whole handshake deadline: 30 s (`HANDSHAKE_TIMEOUT`).

## 4. Inner channel

```
secret = Paired::export("scrin gateway channel v1")          // 32 bytes
(sealer, opener) = scrin_crypto::channel::lane(&secret, side, lane, ordering)
```

- `side` = `Side::Controller` in the browser, `Side::Host` on the host.
- Stream lanes are `Ordering::Ordered`; the datagram lane is `0xFFFFFFFF`, `Ordering::Unordered`
  (64-wide replay window), shared by both directions (keys differ per direction).
- **Control stream**: after the controller sends `Result(0)` (and the host receives it) every
  further frame body on the Control stream is `seal(lane 0, envelope)`; the plaintext is one
  protobuf `scrin.v1.Envelope`, exactly as on the native path.
- **Other streams**: every frame body is `seal(lane, envelope)` from the first frame.
- **Datagrams**: each datagram is `seal(0xFFFFFFFF, shard)` where `shard` is one
  `scrin_media::fec` shard (16-byte `ShardHeader` + payload). Sealing adds 24 bytes: a 1166-byte
  shard becomes a 1190-byte datagram (≤ 1200).
- A frame that fails to open is fatal for its stream (close the session with `PROTOCOL`); a
  datagram that fails to open is dropped and counted.
- Sealed format: `counter u64 BE || ciphertext || tag(16)`; nonce = `0u32 || counter`.

## 5. Session (after pairing)

Unchanged from the native engine, on the sealed Control stream:

1. C→H `SessionRequest` (browser requests `VIEW, INPUT, CLIPBOARD, CHAT`).
2. Both sides show the SAS (`Paired::sas`, 5 indices into `scrin_crypto::sas::EMOJI`); the host's
   interstitial shows it next to Accept/Reject. A browser user who reports a mismatch sends
   `SessionEnd` and closes with application code `0x03`.
3. H→C `SessionAccept` / `SessionReject`, `PermissionsUpdate`, `SessionEnd`.
4. H→C `VideoConfig` (H.264 Annex B, SPS/PPS in band in every keyframe, `codec_config` empty), then
   video shards on datagrams.
5. C→H `BitrateFeedback` every 50 ms, `KeyframeRequest` after a decoder error or a lost
   reference, `Ping` every 1 s (host answers `Pong`).
6. Input: C opens the Input stream (`01 00 00`) and sends `KeyEvent`, `MouseMove` (absolute,
   normalised to the display in `VideoConfig.display_id`; relative under pointer lock),
   `MouseButton`, `MouseWheel` (120 units per notch, positive = away from the user / right).

## 6. Close codes (application, < 0x100)

| code | meaning |
|---|---|
| `0x00` | normal end |
| `0x01` | protocol error (bad frame, failed open, unexpected message) |
| `0x02` | pairing failed (wrong code, bad signature, id mismatch) |
| `0x03` | SAS mismatch reported by the user |
| `0x04` | handshake timeout |

## 7. Host-side checklist (`scrin-engine`, not implemented yet)

- Accept iroh connections with ALPN `scrin-gw/1` only when the host allows browser access.
- Ignore `remote_device_id(conn)` (it is the gateway). Read the 3-byte header of the first bidi
  stream; it must be `00 00 00`.
- Run §3 with the current `HostCode`; treat the session as `SessionKind::Anonymous` with the
  ADR-0009 caps; feed the result into the same host state machine as a native pairing.
- Wrap the Control writer/reader and the Input reader with lane 0 / lane `0x00010000`; seal every
  outgoing datagram on lane `0xFFFFFFFF`.
- Run `cargo test -p scrin-wasm --test vectors` semantics on the host implementation: the vectors
  pin the handshake bytes, both confirmation tags, the SAS, the channel secret, the `Attest`
  signatures, sealed control/input/datagram samples and the protobuf encodings used by the browser.
