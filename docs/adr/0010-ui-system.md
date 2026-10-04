# ADR-0010: UI system — one React UI package, theme engine, i18n

Date: 2026-10-04 · Status: Accepted · Tracker: D12, D20, D10

## Context

The same screens (connect, session toolbar, address book, devices, settings, org console) appear
in the desktop app (Tauri webview, ADR-0002) and the browser (client + console). Users asked for a
modern, fast, accessible UI with rich theming (D20) in English and Romanian. Android is native
Compose (ADR-0006) but must look like the same product. dashy already has a theme engine to reuse.

## Decision

- **`packages/ui`**: shadcn-style components owned in-repo, built on **Base UI**
  (`@base-ui/react`; `render=` composition, `data-open` states), CVA + tailwind-merge,
  Tailwind CSS 4 CSS-first tokens, lucide icons, sonner toasts.
- **Consumers**: `apps/desktop` (Tauri 2 webview) and `apps/web` — one **Vite 8 SPA** with
  **TanStack Router** + TanStack Query for both the browser client and the account console (D12).
- **`ScrinHost` interface** isolates platform differences: `WebHost` (WebTransport, WebCodecs,
  WebCrypto) and `DesktopHost` (Tauri commands/events to the native engine). Components never
  import Tauri or browser-only APIs directly.
- **Theme engine** (from dashy): light / dark / system, user **OKLCH accent** with derived ramps,
  surface **solid / glass / AMOLED**, density (compact/comfortable), motion scale, honours
  `prefers-reduced-motion`. **Automated AA contrast tests** over every theme combination.
- **i18n**: i18next, EN + RO from day one in `packages/i18n`; a drift test fails when any key is
  missing in either locale.
- **Motion**: Motion 14 with `LazyMotion`, React `<ViewTransition>` for route changes, skeletons for
  every async view; animate only `transform`/`opacity` (compositor-only), never layout properties.
- **Layout**: container queries; Playwright visual + axe runs from **360 px wide to 5120×1440
  (32:9)**.
- **Android**: Material 3 Compose theme **generated** from the same tokens (Kotlin source emitted
  by a script; a drift test compares it to the token file).
- **Site**: `apps/site` on Astro 7 + Starlight (static), sharing tokens, not components.

## Consequences

- One component library, one set of tests and stories for desktop and web.
- The webview never renders remote video (desktop) — UI performance is decoupled from streaming.
- Base UI is newer than Radix; some shadcn snippets need adaptation.
- Theme combinations multiply test cases; contrast checks are computed, visual snapshots only for
  representative combinations.
- Adding a locale is adding a file plus passing the drift test.

## Alternatives considered

- **Radix primitives** — legacy in this stack (shadcn moved to Base UI); mixing both is banned.
  Rejected.
- **Per-surface UI** (separate desktop and web component sets) — double the work and drift.
  Rejected.
- **Next.js for the web client** — SSR/RSC adds nothing to an authenticated real-time client, and a
  static SPA is easier to self-host and reuse in Tauri. Rejected; Astro covers the static site.
