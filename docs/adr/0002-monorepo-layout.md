# ADR-0002: One monorepo, shared Rust core, native video on desktop

Date: 2026-10-04 · Status: Accepted · Tracker: D06, D10, D11, D12

## Context

scrin ships on Windows (host + client), Android (host + client, TV), the browser (client +
console), a server binary (rendezvous, relay, gateway) and an accounts API, plus SDK, CLI and MCP.
The protocol, crypto, session state machine and FEC must behave identically on every surface;
a divergence between two implementations of the pairing handshake is a security bug, not a style
issue. The project reuses proven patterns from titi (Rust core + UniFFI + gates) and dashy (iroh
pairing, theme engine) (D06).

Desktop has a hard latency requirement: video must reach the screen without passing through a
WebView. WebView2 throttles background/occluded content, adds a copy into the compositor and
cannot present a D3D11 texture directly.

## Decision

One repository, four toolchains, one gate script:

```
proto/scrin/v1/*.proto   wire contract, buf lint + buf breaking
crates/                  Cargo workspace (Rust edition 2024)
  scrin-proto            prost types + hand-packed media header
  scrin-crypto           identity, SPAKE2, SAS, trust, audit chain
  scrin-net              iroh endpoint, ALPN, framing, datagram FEC, BWE
  scrin-media            codec traits, packetizer, Reed-Solomon, pacing, Opus
  scrin-session          pure state machine (on_event/on_tick -> actions), permissions
  scrin-win              DXGI, Media Foundation, SendInput, WASAPI, clipboard, VDD
  scrin-engine           native host/client engine used by the desktop app
  scrin-ffi              UniFFI bindings (Android)
  scrin-wasm             wasm-bindgen: handshake, protocol, FEC (browser)
  scrin-server           rendezvous + relay + gateway
  scrin-service          Windows SYSTEM service + per-session agent
apps/{desktop,web,api,site}            pnpm workspace
packages/{ui,i18n,config,protocol,sdk,cli,mcp}
android/{app,tv,core-ffi}              Gradle, version catalog
infra/terraform, deploy/               GCP + docker compose self-host
```

- **Dependency arrows point inward.** `scrin-proto` and `scrin-crypto` depend on nothing in the
  workspace; `scrin-session` is pure (no I/O, no clock — time is an input) so it is tested
  deterministically; platform crates (`scrin-win`, `scrin-ffi`, `scrin-wasm`) depend on the core,
  never the reverse. A gate invariant checks the crate graph.
- **Desktop = Tauri 2 shell for UI only + native Rust engine** (`scrin-engine`). The remote
  screen is a child window with its own D3D11 swapchain placed under/over the webview region;
  decoded textures are presented directly. The webview draws chrome, toolbars and dialogs and
  talks to the engine through Tauri commands/events (D10).
- Android uses the same Rust core through `scrin-ffi` (ADR-0006); the browser uses it through
  `scrin-wasm`. Protocol and crypto are written once.
- Web client and console are one Vite SPA sharing `packages/ui` with the desktop app (D12,
  ADR-0010).
- `scripts/gates.ps1` runs rust, js, android, proto, size, e2e and invariants lanes; CI has one
  required `gate` job.

## Consequences

- One PR can change the proto, the Rust core, the TS types and the Kotlin bindings together;
  `buf breaking` stops accidental wire breaks.
- CI must be path-filtered or it becomes slow; the gate lanes run in parallel.
- Contributors need Rust, Node 24 + pnpm, and (only for Android) JDK + NDK. Lanes skip cleanly
  when a toolchain is absent locally; CI runs all.
- The native child-window approach means window-management code per OS (Win32 now,
  NSWindow/X11/Wayland later). That is the price of zero-copy presentation.
- Many agents and humans edit the same tree; ownership by directory keeps edits disjoint.

## Alternatives considered

- **Electron** — 100+ MB, Chromium per app, same problem of video inside a web surface.
  Rejected.
- **Tauri with video in the WebView** (WebCodecs/canvas inside WebView2) — simplest, but WebView2
  throttling, an extra GPU copy and no control over present timing add latency and jank.
  Rejected for desktop (still the path for the browser client, where there is no choice).
- **Polyrepo** (core, server, apps, android separate) — protocol changes become multi-repo
  version dances; titi and dashy showed the monorepo works. Rejected.
- **Flutter for all clients** — one UI toolkit, but no reuse with the web console and a weaker
  path to native D3D11 presentation; Android would still need the Rust core. Rejected.
