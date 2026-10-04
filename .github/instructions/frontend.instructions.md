---
applyTo: 'apps/web/**, apps/desktop/src/**, packages/**/*.{ts,tsx,css}'
---

# Frontend (web, desktop shell, shared packages)

## Stack

- `apps/web`: Vite 8 SPA, React 19.3, TanStack Router + TanStack Query 5. One SPA for the browser
  client and the account console. `apps/desktop`: Tauri 2.12 shell — UI only; video is drawn natively
  by `scrin-engine`, never inside the WebView.
- Platform differences go behind the `ScrinHost` interface (`WebHost`, `DesktopHost`). Components never
  import `@tauri-apps/*` directly.
- TypeScript strict + `noUncheckedIndexedAccess` + `exactOptionalPropertyTypes`. No `any`, no `as`
  casts to silence errors, no non-null `!` on data you did not just check.
- Validation on the client with `zod/mini`; server contracts come from `packages/protocol` / `packages/sdk`.

## Components (`packages/ui`)

- shadcn-style components built on **Base UI** (`@base-ui/react`). **Never import Radix** (`radix-ui`,
  `@radix-ui/*`) — the invariants check fails the build.
- Base UI idioms: `render={...}` instead of `asChild`, style state with `data-open` / `data-disabled`
  attributes, not `data-state`.
- Variants with CVA + `cn()` (clsx + tailwind-merge). Icons from `lucide-react`. Toasts via `sonner`.
- Every async view has a skeleton; every mutation has pending, success and error states.

## Styling and motion

- Tailwind v4 CSS-first: tokens in `@theme` (OKLCH), no `tailwind.config.js`. No hard-coded colours
  in components — use tokens so light/dark/AMOLED/glass surfaces and the accent engine work.
- Motion 14 via `motion/react` only (never `framer-motion`), loaded through `LazyMotion` + `m.*`.
- Animate compositor-only properties: `transform`, `opacity`, `filter`, `clip-path`. Never animate
  `width`, `height`, `top`, `left`, `margin` or `box-shadow`.
- Respect reduced motion (`useReducedMotion` / `prefers-reduced-motion`) and the theme's motion scale;
  route changes use `<ViewTransition>`.
- Layout with container queries; test from 360 px wide up to 5120×1440 (32:9).

## i18n

- i18next with keys in `packages/i18n` — **every key in both `en` and `ro`** in the same commit; the
  drift test fails otherwise. No user-visible string literals in JSX (aria-labels included).
- Romanian uses proper diacritics (ș ț with comma below, ă â î). Dates/numbers via `Intl` with the
  active locale. Use the `add-locale-string` skill.

## Accessibility (WCAG 2.2 AA)

- Every interactive element is keyboard reachable with a visible focus ring; targets ≥ 24×24 px.
- Inputs have labels; icon-only buttons have an i18n `aria-label`; dialogs trap and restore focus.
- Contrast ≥ 4.5:1 text, 3:1 UI — the theme contrast tests cover every surface/accent combination.
- Live session status (connected, quality, permission changes) is announced via an `aria-live` region.

## Hygiene

- No `console.log` (oxlint + eslint `no-console`); use the logger in `packages/config` if one exists.
- No secrets, codes or keys in local storage, URLs or logs; device secrets use non-extractable WebCrypto.
- Verify: `pwsh -NoProfile -File scripts/gates.ps1 -Only js,size,e2e` (axe runs in e2e).
