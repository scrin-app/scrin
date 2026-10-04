# scrin — architecture

Open-source (AGPL-3.0-only) remote desktop. Quick connect with **ID + one-time code**, unattended
access, gaming-grade streaming, and teams/MSP management. Decisions with their reasons are in
`docs/adr/` and the decision log in `docs/TRACKER.md` § Decisions.

## 1. Surfaces

| Surface | Role | Tech |
|---|---|---|
| `apps/desktop` | Windows client + host (later macOS/Linux) | Tauri 2.12 shell (UI only) + native Rust engine (`scrin-engine`) |
| `crates/scrin-service` | Windows SYSTEM service + per-session agent (UAC, login screen, unattended) | Rust, windows-rs |
| `android/` | Android client + attended host, Android TV client | Kotlin 2.4, Compose BOM 2026.09, navigation3, Rust core via UniFFI |
| `apps/web` | Browser client + account console (one SPA) | Vite 8, React 19.3, TanStack Router/Query, WebCodecs, WebTransport |
| `apps/api` | Accounts, orgs, address book, devices, policies, audit, billing, webhooks | Hono 4 + `@hono/zod-openapi`, Drizzle 0.45, PostgreSQL 17, better-auth (passkeys) |
| `crates/scrin-server` | Rendezvous (ID registry) + iroh relay + WebTransport/WebSocket gateway | Rust, iroh-relay 1.3, wtransport/web-transport-quinn, axum |
| `apps/site` | Marketing + docs | Astro 7 + Starlight |
| `packages/sdk`, `packages/cli`, `packages/mcp` | Public TS SDK (from OpenAPI), CLI, MCP server | tsdown |

## 2. Monorepo layout

```
proto/scrin/v1/*.proto          wire contract (buf lint + buf breaking in CI)
crates/
  scrin-proto      prost types + hand-packed media header (hot path, no protobuf)
  scrin-crypto     device identity, SPAKE2 pairing, SAS, trust store, audit hash-chain
  scrin-net        iroh endpoint, ALPN, framing, stream kinds, datagram FEC, BWE
  scrin-media      codec traits, packetizer, Reed-Solomon FEC, frame pacing, Opus
  scrin-session    session state machine (pure: on_event/on_tick -> actions), permissions
  scrin-win        Windows: DXGI capture, MF encoder/decoder, SendInput, WASAPI, clipboard, VDD
  scrin-engine     native engine used by desktop (host + client roles)
  scrin-ffi        UniFFI bindings for Android
  scrin-wasm       wasm-bindgen: handshake, protocol, FEC for the browser
  scrin-server     rendezvous + relay + gateway binary
  scrin-service    Windows service + agent binaries
apps/{desktop,web,api,site}
packages/{ui,i18n,config,protocol,sdk,cli,mcp}
android/{app,tv,core-ffi}
infra/terraform  GCP (Cloud Run api, Cloud SQL, GCE relay VMs, DNS)
deploy/          docker-compose self-host bundle
```

## 3. Identity and security

- **Device identity**: Ed25519 keypair per install = iroh `EndpointId`. Secret sealed with DPAPI
  (Windows), Android Keystore (wrapping key), non-extractable WebCrypto + IndexedDB (web).
- **scrin ID**: 9-digit random number, bound on the rendezvous server to the device public key;
  re-registration requires a signature by that key. Every server RPC is signed.
- **Quick connect**: the host shows ID + one-time code (8 chars, 30-symbol alphabet, 10 min, single
  use). The controller dials the host over iroh; inside the QUIC channel both run **SPAKE2** on the
  code, bound to both endpoint ids and the ALPN (`scrin/1`). The code never travels. Both screens
  show a **SAS** (5 emoji from the transcript) for verbal verification.
- **Unattended**: host-side trust list of controller device keys (added in an attended session or by
  org policy, which the host verifies by signature). Optional permanent password: Argon2id verifier
  only, checked through SPAKE2. Optional TOTP/passkey second factor on the controller account.
- **Relay is blind**: iroh relays forward QUIC ciphertext. The browser gateway terminates
  WebTransport TLS but the session inside is protected by an inner Noise/SPAKE2 handshake to the host
  key, so the gateway sees only ciphertext.
- **Permissions** per session (view, input, clipboard, files in/out, audio, mic, restart, terminal,
  record, privacy, block input, tunnel), granted by the host, revocable live.
- **Anti-scam**: pre-accept interstitial, verified/unverified badge, anonymous-session caps (60 min, no
  files, no unattended setup, no privacy mode), sensitive-app blur, Stop & report.
- **Audit**: append-only, `prev_hash` + Ed25519 signature per record, anchored to the server.
- **Updates**: Tauri updater with minisign manifest (key separate from code signing).

## 4. Transport

- **Native ↔ native**: iroh 1.3 (QUIC via noq, hole punching, relay fallback over HTTPS 443).
  - Reliable streams per kind: control, input, clipboard, files (one per file), chat, tunnel.
  - **Video/audio over QUIC datagrams** with a fixed 16-byte media header, ≤1200-byte shards,
    adaptive Reed-Solomon FEC (5–30 %), keyframe/intra-refresh request on unrecoverable loss.
  - App-level delay-based bandwidth estimation (GCC-style trendline) driving encoder bitrate,
    then fps, then resolution.
- **Browser ↔ host**: WebTransport (HTTP/3) to `scrin-server` gateway, which dials the host over
  iroh and forwards streams/datagrams. Fallback: WebSocket over TLS 443.
- **Self-hosted relays** on GCE (europe-west) from day one; n0 public relays only in development.

## 5. Media pipeline

Host (Windows): DXGI Desktop Duplication (GPU texture, dirty rects, cursor separate) in the agent
running in the active session → Media Foundation hardware MFT (H.264/HEVC/AV1 negotiated) or
openh264 → packetizer + FEC → datagrams. Audio: WASAPI process-excluding loopback → Opus 10 ms.
Cursor shape/position sent as its own channel and drawn client-side.

Client: native = MF decoder → D3D11 swapchain in a child window under the Tauri webview; Android =
MediaCodec low-latency → SurfaceView; web = WebCodecs `VideoDecoder` → WebGPU/WebGL2 canvas,
`AudioDecoder` → AudioWorklet. Latest-frame-wins presentation, no video jitter buffer.

Codec negotiation: client sends decode capabilities (`isConfigSupported` on web, MediaCodecList on
Android, MF enumeration on Windows); host intersects with its encoders. Order: AV1 > HEVC > H.264.

## 6. Input

Physical scancodes (`KeyboardEvent.code` / HID usage) by default, Unicode translate mode for layout
mismatch, relative mouse for games, Ctrl+Alt+Del via `SendSAS`, gamepads via ViGEmBus 1.22
(optional component). Android host input via AccessibilityService gestures + global actions.

## 7. UI system

One React UI package (`packages/ui`, shadcn-style components on Base UI) used by desktop (Tauri) and
web. Platform differences behind a `ScrinHost` interface (`WebHost`, `DesktopHost`). Theme engine:
light/dark/system, OKLCH accent, surface (solid/glass/AMOLED), density, motion scale, reduced
motion; AA contrast tests. i18n: i18next, EN + RO, drift tests. Motion 14 (`LazyMotion`),
`<ViewTransition>` for route changes, skeletons for every async view. Layout: container queries,
tested at 360 px → 5120×1440 (32:9). Android: Material 3 Compose with the same tokens exported as
a generated Kotlin theme.

## 8. Backend (accounts)

Hono + zod-openapi on Cloud Run, Cloud SQL Postgres 17, better-auth (email+passkey, TOTP, OIDC for
orgs). Entities: user, org, membership/role, device, device_group, address_book_entry, policy,
session_log, audit_event, jit_grant, webhook, branding. Email and billing through brivio.
OpenAPI → `packages/sdk` → CLI and MCP.

## 9. Quality gates

`scripts/gates.ps1` lanes (parallel): rust (fmt, clippy -D warnings, test, cargo-deny), js (oxlint
type-aware, eslint, tsc, vitest), android (ktlint, lint, unit, assemble), proto (buf lint/breaking),
size (bundle + binary + APK budgets), e2e (Playwright + axe), invariants (scripted repo rules,
mutation-tested). lefthook: pre-commit fast subset, pre-push gates. CI: path-filtered jobs + one
required `gate` job.
