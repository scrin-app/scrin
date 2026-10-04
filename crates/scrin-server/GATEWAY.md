# scrin browser gateway — framing contract

The gateway lets a browser reach a scrin host that only speaks iroh QUIC. It is a **dumb
pipe**: it never parses, decrypts or rewrites application bytes. The browser and the host run
the scrin handshake (version `Hello`, SPAKE2 pairing or trusted auth, SAS) and then the inner
end-to-end channel (`scrin_crypto::channel`, ChaCha20-Poly1305 per lane) **through** it. The
one-time code never reaches the gateway; it only ever sees ciphertext and stream boundaries.

This document is the contract for the host-side handler of ALPN `scrin-gw/1` (implemented in
the host crates) and for the web client.

## 1. Session setup

| Transport | URL | Server listener |
|---|---|---|
| WebTransport (HTTP/3) | `https://<server>:<port>/v1/gw?id=<scrin id>` | UDP (`--wt-listen`, default = TCP port) |
| WebSocket fallback | `wss://<server>/v1/ws?id=<scrin id>` | TCP (`--listen`) |

`id` is the 9-digit scrin ID (no separators). On a new session the gateway:

1. applies the per-IP gateway rate limit and the per-target resolve limit, and the
   pairing-failure lockout of the target (same rules as an anonymous `GET /v1/resolve/{id}`);
2. looks up the host's device key + address hint (presence must be fresh, 60 s TTL);
3. dials the host over iroh with **ALPN `scrin-gw/1`**, using the host's relay URL and direct
   addresses from the hint, with a 10 s timeout;
4. only then accepts the browser session.

Failures before acceptance:

| Condition | WebTransport | WebSocket (HTTP response, no upgrade) |
|---|---|---|
| malformed / missing id | 404 | 400 `bad_request` |
| rate limited / target locked out | 429 | 429 `rate_limited` / `locked_out` |
| host offline, unknown or dial failed | 404 | 404 `offline` / 502 `unreachable` |

The gateway's iroh endpoint uses a fresh random key per process. **The host must not treat the
gateway's endpoint id as the controller's identity**: the controller is identified only by the
inner handshake. A host should accept `scrin-gw/1` connections only when it is willing to be
reached from browsers, and must apply the same anti-scam policy (ADR-0009) as for any anonymous
controller.

## 2. WebTransport mapping (1:1)

Every WebTransport stream maps to exactly one iroh QUIC stream on the gateway↔host connection,
and every datagram to one datagram:

| Browser (WebTransport) | Host (iroh, `scrin-gw/1`) |
|---|---|
| browser opens bidi stream | gateway opens bidi stream to host |
| host opens bidi stream | gateway opens bidi stream to browser |
| browser opens uni stream | gateway opens uni stream to host |
| host opens uni stream | gateway opens uni stream to browser |
| datagram | datagram (same bytes) |

- Stream bytes are copied in order, unchanged. Stream ids are **not** preserved (each QUIC
  connection numbers its own streams); the stream *pairing* and *order of opening* are. Hosts must
  not rely on stream ids, only on the stream-kind frame each stream starts with (as on the native
  path, `scrin_net::framing`).
- **FIN** on one side → `finish()` on the other.
- **RESET_STREAM(code)** → reset with the same code. **STOP_SENDING(code)** on a write half →
  the gateway stops the matching read half with the same code.
- Datagrams are forwarded best-effort. Payloads larger than the other side's
  `max_datagram_size` are dropped silently (the media layer already keeps shards ≤ 1200 B).

## 3. WebSocket fallback framing

Binary messages only (a text message closes the session with `PROTOCOL`). Each message is one
frame: a 1-byte tag, then QUIC varints (RFC 9000 §16):

| tag | frame | body | meaning |
|---|---|---|---|
| `0x00` | `Data` | varint stream id, payload | bytes on a stream |
| `0x01` | `Dgram` | payload | one datagram |
| `0x02` | `Fin` | varint stream id | sender finished its write half |
| `0x03` | `Reset` | varint stream id, varint code | sender reset its write half |
| `0x04` | `Stop` | varint stream id, varint code | sender stops reading (STOP_SENDING) |

Stream ids follow QUIC's numbering *on the WebSocket* (they are virtual; the host still sees
normal iroh streams):

- bit 0 = initiator: `0` browser, `1` host (via gateway); bit 1 = `0` bidi, `1` uni.
- Browser-initiated ids: bidi `0, 4, 8, …`, uni `2, 6, 10, …`. A `Data`/`Fin`/`Reset` with a
  new browser id **opens** the stream (the gateway opens the matching host stream). Ids must be
  increasing per type; a reused or lower id is ignored (stream already closed).
- Host-initiated ids are allocated by the gateway: bidi `1, 5, 9, …`, uni `3, 7, 11, …`; the
  first `Data` (or `Fin`) frame for such an id announces the stream to the browser.
- An empty `Data` frame is legal (opens a stream without bytes).
- Max message size 256 KiB; max 256 concurrent streams per session (more closes with `QUOTA`).
- Datagrams over WebSocket are reliable and ordered (TCP); they are still subject to the
  bandwidth cap and are dropped, not delayed, when the session is over its rate.

## 4. Close codes

Codes `< 0x100` belong to the application (browser and host) and are passed through unchanged
in both directions: a WebTransport session closed by the browser with code `c` closes the
host connection with application code `c`, and vice versa. Codes from `0x100` are the
gateway's own:

| code | name | when |
|---|---|---|
| `0x000` | `NORMAL` | normal close |
| `0x101` | `HOST_UNREACHABLE` | dial failed after accept (rare; normally reported before accept) |
| `0x102` | `SESSION_LIMIT` | maximum session length reached (anonymous cap, default 60 min, ADR-0009) |
| `0x103` | `IDLE` | no forwarded bytes for `--gw-idle-secs` (default 120 s) |
| `0x104` | `QUOTA` | byte cap exceeded, or too many WS streams |
| `0x105` | `PROTOCOL` | malformed WebSocket frame / text message |
| `0x106` | `SHUTDOWN` | server shutting down |
| `0x107` | `BROWSER_GONE` | browser transport dropped without a close code |
| `0x108` | `HOST_GONE` | host connection lost without an application close |

On WebSocket the close code is `4000 + code` (e.g. `IDLE` → `4259`). A browser closing a
WebSocket with `4000 + c` (`c < 1000`) propagates `c` to the host; any other close code
propagates as `BROWSER_GONE`.

## 5. Quotas

Per session, configurable on the server:

| flag | env | default |
|---|---|---|
| `--gw-max-secs` | `SCRIN_GW_MAX_SECS` | 3600 (60 min) |
| `--gw-idle-secs` | `SCRIN_GW_IDLE_SECS` | 120 |
| `--gw-max-bps` | `SCRIN_GW_MAX_BPS` | 3 000 000 B/s (both directions combined; 0 = off) |
| `--gw-max-bytes` | `SCRIN_GW_MAX_BYTES` | 0 (off) |

Stream bytes over the rate are **delayed** (back-pressure); datagrams over the rate are
**dropped**, so the media layer's bandwidth estimator sees loss and lowers the bitrate.

v1 treats every gateway session as anonymous (60-minute cap). Verified-account sessions without
the cap need the accounts API to issue a token the gateway can check; that is not part of this
contract yet.
