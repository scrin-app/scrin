# scrin — canonical tracker

Single source of truth for goals, decisions and status. The row-level backlog lives in
`docs/tracker.csv` (same ids). A row is `done` only with evidence (command + result, test name,
screenshot path). Statuses: `todo` · `doing` · `done` · `blocked` (reason) · `dropped` (reason).

## Goal

An open-source (AGPL-3.0-only) remote desktop for every OS, mobile and desktop: connect in seconds
with an ID + one-time code, plus unattended access, gaming-grade streaming and teams/MSP features,
with a modern, fast, accessible UI.

## Done means (v1)

1. Windows ↔ Windows: quick connect by ID + code across two real networks (PC ↔ dragos-vivobook),
   control including UAC and login screen, audio, clipboard, files — VERIFIED live.
2. Android A51: client to a Windows host, and attended host controlled from Windows — VERIFIED live.
3. Browser (Chrome, Firefox, Safari 26) client to a Windows host through the gateway — VERIFIED.
4. Self-host bundle (`docker compose up`) works from a clean machine; official instance on GCP.
5. `scripts/gates.ps1` green: zero lint/type warnings, tests, size budgets, axe, buf breaking.
6. Every csv row is `done` with evidence, or `dropped`/`blocked` with a stated reason.

## Delivery order (verified slices)

| Slice | Content | Exit check |
|---|---|---|
| S0 Foundation | repo, toolchains, gates, hooks, CI, agent docs | `gates.ps1` green on an empty-but-real workspace |
| S1 Core | proto, crypto (identity, SPAKE2, SAS, trust), net (iroh, framing, FEC), session state machine | two engines pair over loopback in a test, SAS equal, wrong code rejected |
| S2 Win stream | DXGI capture → MF encode → datagrams → MF decode → window; input back | PC ↔ vivobook live, latency stats shown |
| S3 Server | rendezvous + relay + gateway + docker compose | connect by ID through self-hosted relay |
| S4 UI | packages/ui + theme + i18n + desktop app screens | Playwright visual + axe at 360 px … 32:9 |
| S5 Web client | WebTransport + WebCodecs | Chrome/Firefox control a Windows host |
| S6 Android | client + attended host | A51 both directions |
| S7 Service | SYSTEM service, UAC, login screen, unattended | lock screen + UAC controlled remotely |
| S8 Accounts | API, console, address book, orgs | sign up with passkey, device appears |
| S9+ | features X-*, MSP, gaming, TV, brand, site | per row |

## Decisions (2026-10-04, with the user)

| # | Decision | Reason |
|---|---|---|
| D01 | Name **scrin**, licence **AGPL-3.0-only**, DCO for contributions | Prevents closed SaaS forks; DCO keeps contribution friction low |
| D02 | v1 platforms: Windows host+client, Android host+client, web client | User choice; macOS/Linux/iOS later |
| D03 | Scope: everything, delivered as verified slices | User wants all; slices make each step provable |
| D04 | Self-hostable server (Docker) + official instance, **all on GCP europe** (GCE VMs for relay) | User chose GCP over Hetzner; watch egress cost (SV-007) |
| D05 | Accounts optional; account = address book, devices, orgs, Pro | Quick connect must work with no account |
| D06 | Reuse dashy (iroh pairing), titi (Rust core + UniFFI + gates), brivio (email, billing) | Proven code |
| D07 | Dedicated GitHub org, public repo | Isolated secrets/signing, clear brand |
| D08 | Android host is **attended** (consent each session); Device Owner optional for unattended | Android 14–16 MediaProjection rules |
| D09 | Web client goes through a WebTransport gateway (no 4:4:4, +5–20 ms) | No P2P in WebTransport; WebRTC later if needed |
| D10 | Desktop = Tauri 2 UI shell + native Rust engine; video drawn natively, not in WebView | WebView2 throttling; latency |
| D11 | Android = Kotlin + Compose + Rust core via UniFFI | Native feel; titi pattern |
| D12 | Web client + console = one Vite SPA sharing `packages/ui`; site = Astro Starlight | One UI codebase; static hosting |
| D13 | Transport = iroh 1.3 + WebTransport gateway + WebSocket 443 fallback | P2P + firewall traversal; Ed25519 identity |
| D14 | Auth = Ed25519 device keys + SPAKE2 + SAS emoji + trust list | No offline attack on short codes, no server MITM |
| D15 | Accounts backend = Hono + zod-openapi + Drizzle + PG17 + better-auth | Golden stack, OpenAPI → SDK/CLI/MCP |
| D16 | Encoder = Media Foundation hardware MFT + openh264; NVENC/AMF later | Small installer, any GPU |
| D17 | Code signing deferred — binaries unsigned until an SRL/PFA exists | User has no legal entity yet; SmartScreen will warn |
| D18 | Android distribution: Play + GitHub Releases + foss flavour (F-Droid) | Reach + FOSS audience |
| D19 | Extras in scope: anti-scam, virtual display + privacy, gaming, mic/chat/whiteboard, recording + audit chain, RDP/SSH/VNC gateway, tunnel/terminal/low-bw, AI summary (codai), MSP, JIT + handoff, net diagnostics, Android TV | User selection (pen pressure not selected) |
| D20 | Themes: light/dark/system, OKLCH accent, surface solid/glass/AMOLED, density, reduced motion; EN + RO | Reuse dashy engine |
| D21 | Brand via Brand Designer flow, in parallel | Icons needed for installers/stores |
| D22 | Test devices: this PC, dragos-vivobook (ssh), Samsung A51 (adb R58N94BMLJY) | Real hardware |
| D23 | Domain `scrin.dragoscatalin.ro` for now | User choice; bundle ids use `ro.dragoscatalin.scrin` |

## Open questions

- Q01: GitHub org name (`scrin-app` vs `scrinhq`) — user creates it; needed for F-012.
- Q02: Signing entity (SRL/PFA) — blocks SmartScreen-clean releases (D17).

## Status log

- 2026-10-04: research (competitors, media/transport, security, dashy/titi reuse, versions) done;
  decisions D01–D23 recorded; S0 started.
