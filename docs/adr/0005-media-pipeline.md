# ADR-0005: Media pipeline — DXGI capture, hardware encode, datagrams with FEC

Date: 2026-10-04 · Status: Accepted · Tracker: D16, D10, D13

## Context

Target: gaming-grade latency on a LAN and graceful degradation on lossy mobile/Wi-Fi links, on any
Windows GPU, with a small installer. Capture must work on the secure desktop (UAC, login screen),
so it runs in a per-session agent launched by the SYSTEM service. Clients are Windows, Android
and browsers, each with a different hardware decoder API.

## Decision

**Capture (host, Windows)**: DXGI Desktop Duplication in the per-session agent. GPU texture, dirty
rects and move rects drive encoder hints; the cursor is a separate channel (shape + position) drawn
client-side so it stays responsive at low video fps. `DXGI_ERROR_ACCESS_LOST` (desktop switch, mode
change, UAC) recreates the duplication and forces a keyframe.

**Encode**: Media Foundation hardware MFT (H.264, HEVC, AV1 as available) in low-latency mode
(`CODECAPI_AVLowLatencyMode`, no B-frames, CBR/VBR with tight VBV, intra refresh where supported).
Software fallback: openh264. Vendor SDKs (NVENC, AMF, QSV direct) later (D16).

**Packetize**: fixed 16-byte hand-packed media header (stream id, frame id, shard index/count,
FEC group, flags incl. keyframe, timestamp) — not protobuf, on the hot path. Shards ≤1200 bytes to
fit any QUIC datagram MTU. **Adaptive Reed-Solomon FEC** 5–30 % parity per frame based on measured
loss. Unrecoverable loss → keyframe or intra-refresh request on the `control` stream.

**Rate control**: app-level delay-gradient bandwidth estimator (GCC-style trendline over
per-shard send/receive timestamps, plus loss) in `scrin-net`. Output drives encoder bitrate first,
then fps, then resolution.

**Audio**: WASAPI loopback excluding scrin's own process → Opus, 10 ms frames, datagrams, small
jitter buffer.

**Decode/present (client)**:
- Windows: MF decoder → D3D11 texture → child-window swapchain (ADR-0002).
- Android: MediaCodec low-latency mode → SurfaceView.
- Web: WebCodecs `VideoDecoder` → WebGPU (WebGL2 fallback) canvas; `AudioDecoder` → AudioWorklet.
- **Latest-frame-wins**, no video jitter buffer: a late frame is dropped if a newer one decoded.

**Negotiation**: client sends decode capabilities (`isConfigSupported`, `MediaCodecList`, MF
enumeration); host intersects with its encoders and picks AV1 > HEVC > H.264.

## Consequences

- No bundled codec libraries beyond openh264 and Opus: small installer, works on any GPU with
  MF drivers; quality/latency tuning depends on vendor MFT behaviour (measured per vendor).
- Desktop Duplication needs the agent in the interactive session — requires the service
  architecture (S7) for UAC/login screen.
- FEC costs bandwidth; adaptive rates keep it near 5 % on clean links.
- Dropping late frames trades smoothness for latency; correct for interactive control, wrong for
  watching video — a "smooth" mode with a small buffer may be added later.
- Hand-packed header must be versioned with the ALPN; `scrin-proto` owns its codec and tests.

## Alternatives considered

- **Windows.Graphics.Capture** — modern API, but yellow border/consent behaviour on some builds
  and no secure-desktop capture. Kept as a per-window capture option later; rejected as primary.
- **Bundling FFmpeg** — broad codec support but 30–80 MB, licence complexity (GPL builds), and MF
  already exposes the hardware encoders. Rejected.
- **Video over reliable QUIC streams** — head-of-line blocking and retransmission delay on every
  loss. Rejected.
- **Full WebRTC media stack** — mature, but huge dependency and its own transport/identity
  (see ADR-0003). Rejected.
