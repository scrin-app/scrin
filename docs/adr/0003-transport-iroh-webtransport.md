# ADR-0003: Transport — iroh QUIC, WebTransport gateway, WebSocket fallback

Date: 2026-10-04 · Status: Accepted · Tracker: D04, D09, D13

## Context

Remote desktop needs: direct peer-to-peer paths when NATs allow it (lowest latency, no relay
egress cost), a relay fallback that passes corporate firewalls (TCP/443 only), both reliable
ordered streams (control, input, files) and unreliable messages (video, audio) on one connection,
and a cryptographic identity per device. Browsers cannot open raw UDP or QUIC to a peer; their
options are WebRTC, WebTransport (HTTP/3, client→server only) and WebSocket.

dashy already pairs devices with iroh (D06), so the team has working code and known pitfalls.

## Decision

- **Native ↔ native: iroh 1.3.** QUIC (via the `noq` stack), Ed25519 `EndpointId` = device key,
  UDP hole punching, automatic fallback to an iroh relay over HTTPS 443. Path migration
  (relay → direct) happens without dropping the session.
- **ALPN `scrin/1`.** A protocol revision bumps the ALPN; peers negotiate the highest common one.
- **Stream kinds** (bidirectional QUIC streams, first frame = kind byte + protobuf header):
  `control`, `input`, `clipboard`, `files` (one stream per file, so a large transfer never
  blocks input), `chat`, `tunnel`. Frames are length-prefixed protobuf (`proto/scrin/v1`).
- **Media over QUIC datagrams**: fixed 16-byte header, ≤1200-byte shards, Reed-Solomon FEC,
  bandwidth estimation in `scrin-net` (details in ADR-0005). No head-of-line blocking for video.
- **Browser ↔ host via gateway** (D09): the browser opens WebTransport (HTTP/3) to
  `scrin-server`; the gateway dials the host over iroh and forwards streams 1:1 and datagrams
  1:1. Inside, the browser and host run their own handshake (ADR-0004), so the gateway relays
  ciphertext. **Fallback: WebSocket over TLS 443** with datagrams emulated as framed messages
  (higher latency under loss, but works through any proxy).
- **Self-hosted relays** on GCE europe-west from day one (ADR-0008); n0's public relays only in
  development builds. The relay URL list is part of server config and of the self-host bundle.
- Rendezvous (ID → `EndpointId` + relay hint) is a separate small service in `scrin-server`; it
  never carries session traffic.

## Consequences

- One connection carries everything; QUIC congestion control covers the reliable streams while
  media uses app-level BWE.
- Browser sessions cost +5–20 ms and gateway egress, and cannot do 4:4:4 at high fps. Accepted
  for v1; WebRTC to the host can be added later if needed (D09).
- iroh is pre-2.0 in spirit even at 1.3; we pin the version and wrap it behind `scrin-net` so an
  upgrade or replacement touches one crate.
- WebTransport availability: Chrome and Firefox ship it; Safari 26 is required for the done
  criteria — the WebSocket fallback covers older Safari.
- Relay and gateway are long-lived UDP/QUIC services — they cannot run on Cloud Run (ADR-0008).
- Self-hosters get the same relay+gateway binary; no dependency on n0 infrastructure.

## Alternatives considered

- **WebRTC everywhere** — native browser support and mature congestion control, but SDP/ICE
  complexity, a signalling server, DTLS identity disconnected from our device keys, and libwebrtc
  is a huge native dependency for desktop/Android. Rejected for v1.
- **Raw quinn + own STUN/TURN** — full control, but we would rebuild hole punching, relaying and
  path migration that iroh already provides and dashy already uses. Rejected.
- **libp2p** — broad but heavy, slower to establish, and its relay/holepunch stack is less proven
  for low-latency media. Rejected.
- **TCP + TLS (RustDesk-style)** — head-of-line blocking makes video stall on every loss; no
  unreliable channel. Rejected.
