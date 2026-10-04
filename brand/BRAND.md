# scrin — brand book

Version **0.1.0** · 2026-10-04 · owner Dragos Catalin Vladulescu · source of truth: `brand/brand.json`
Generated assets come from `brand/scripts/` — edit `palette.mjs`, `geometry.mjs` or
`tokens/motion.tokens.json`, then rebuild (see § Governance). Never hand-edit anything under `dist/`,
`logo/`, `android/` or `concepts/`.

## Platform

**Purpose.** Let anyone reach any screen they are allowed to reach — a parent's laptop, a client's
server, a gaming rig — in seconds, without trusting a middleman with what they see.

**Positioning.** For home helpers, IT technicians, gamers and MSPs who want TeamViewer-level
convenience without the licence nags, closed code or "trust us" security. Against: closed remote
desktops (TeamViewer, AnyDesk) and fiddly self-hosted ones.

**Personality — this, not that**

| This | Not that |
|---|---|
| trustworthy — shows its proof (emoji SAS, open code) | reassuring — "military-grade" claims |
| fast — gets out of the way | frantic — speed lines, neon, urgency |
| calm — one accent, quiet surfaces | sterile — grey enterprise dashboards |
| modern — precise geometry, variable type | trendy — gradients-for-everything, AI sparkle |
| open — plain words, visible source | hacker-edgy — terminals, skulls, green-on-black |

**Meaning of the mark.** *scrin = two screens becoming one.* The lowercase **s** is drawn as two
identical halves turned to face each other — **you** (ivory) and **them** (lagoon) — meeting in the
middle bar. It is the only shape that is simultaneously the initial, a connection and a handshake.

## Voice & tone

Principles: say what happens, in the order it happens · numbers over adjectives · never blame the
user · security claims only when they can be checked.

| Context | EN | RO |
|---|---|---|
| Tagline | Connect to any screen, in seconds. | Conectare la orice ecran, în câteva secunde. |
| Onboarding | Share this ID and code. They stay valid for 10 minutes. | Trimite ID-ul și codul. Sunt valabile 10 minute. |
| Verification | Check that you both see the same five emoji. | Verificați că vedeți aceleași cinci emoji. |
| Error | Couldn't reach that device. It may be offline — try again or check the ID. | Dispozitivul nu răspunde. Poate fi offline — reîncearcă sau verifică ID-ul. |
| Docs | Plain, second person, present tense. | Persoana a II-a, prezent, fără anglicisme evitabile. |
| Incidents | What broke, since when, what you should do, next update time. | Ce s-a stricat, de când, ce ai de făcut, ora următoarei actualizări. |
| Billing (brivio) | Exact amounts with currency; no "only". | Sume exacte cu monedă (RON / EUR); fără „doar”. |

RO uses informal *tu* in the app, *dumneavoastră* nowhere; diacritics are mandatory and use the
comma-below forms **ș ț Ș Ț** (U+0219/021B/0218/021A), never cedilla ş ţ.

## Naming

- Always lowercase: **scrin** — also at sentence start and in titles. Never "Scrin", "SCRIN", "ScrIn".
- Features are plain nouns: *Quick connect*, *Unattended access*, *Teams*. No "scrin Pro Max".
- Pronunciation note for EN docs: /skriːn/, like "screen". In RO it echoes *ecran*; it is not a
  dictionary word, so there is no unwanted meaning (checked: "scrin" is an archaic RO word for a chest of
  drawers / casket — harmless, and a nice "keeps things safe" echo; no slang hits in EN/RO).

## Logo system

| Asset | File | Use |
|---|---|---|
| App icon, master (≥ 64 px) | `logo/app-icon-master.svg` | store, splash, 128–1024 px |
| App icon, small (24–48 px) | `logo/app-icon-small.svg` | taskbar, Start, launcher |
| App icon, micro (16–20 px, pixel grid) | `logo/app-icon-micro.svg` | favicon, title bar, tray |
| Symbol on dark / light (3 optical sizes) | `logo/symbol-{master,small,micro}-on-{dark,light}.svg` | in-app header, docs |
| One-colour (gap at the seam) | `logo/symbol-*-mono-{black,white}.svg` | Android themed icon, stamps, print |
| Inverse tile | `logo/app-icon-inverse.svg` | on lagoon or photographic backgrounds |
| Staging / dev tile | `logo/app-icon-staging.svg` | non-production builds only |
| Wordmark (lettered, not typed) | `logo/wordmark-{on-dark,on-light,mono-*}.svg` | when the symbol is already present |
| Horizontal lockup | `logo/lockup-horizontal-on-{dark,light}.svg` | site header, README, OG, TV banner |
| Construction | `logo/construction.svg` | reference |

**Construction** (`geometry.mjs`): 24-unit keyline grid; one stroke unit = 3 (master 2.75, micro 2
on a 16-unit pixel grid); bars at y 5.5 / 12 / 18.5; stems at x 7.5 / 16.5; outer box 12 × 16; corner
radius 3 (the UI's `radius-md` family); counters 3.5 → stroke:counter 1 : 1.17. 180° rotational
symmetry about (12, 12); the halves meet at x = 12, the ivory half tucks 0.35 under the lagoon half so
no seam shows in colour; one-colour versions open a 1-unit gap there so the two-part idea survives
without colour. Square stroke ends; round joins.

**Wordmark**: drawn from the same strokes (stroke 3, x-height 16, r 3): s · c · r · i · n, hand-kerned
(c/r open sides pull the next letter in). The i-dot is a tiny screen (3 × 3, r 0.75) in the accent.
Verified at 20 px x-height: 5 separate letters, visible gaps 4/5/3/4 px (`sheet/legibility.txt`).

## Clear space & minimum sizes

- Clear space around the symbol = **one middle-bar height** (¼ of the symbol height) on every side;
  around the lockup = the height of the wordmark x-height.
- Minimum: symbol 16 px (use micro), app tile 16 px (micro), lockup 72 px wide, wordmark 48 px wide.

## Misuse

Don't: recolour the halves to anything but ivory/lagoon (or ink/lagoon-deep on light, or a single
colour with the gap) · swap which half is lagoon · rotate, skew, outline or add a glow · put the full-
colour mark on the lagoon colour · type "scrin" in a font instead of using the wordmark · add a
sparkle/AI glyph · animate it with anything except the motion set below.

## Colour

Hue **178 "lagoon"** (aqua-green) on **ink** (deep sea-teal). Chosen because the category is blue
(TeamViewer #0e8ee9, Parsec, Chrome Remote Desktop, Splashtop) or red (AnyDesk #ef443b, RustDesk
blue-grey); lagoon is unowned there, reads as calm/clean, and keeps distance from OpenAI #10a37f
(hue ≈ 165, darker) and Perplexity (≈ 195, desaturated).

### Brand colours (mark, icon, marketing)

| Token | OKLCH | Hex | Role |
|---|---|---|---|
| `brand.ink` | 0.235 0.040 205 | `#002327` | icon plate, OG/TV background |
| `brand.ink-hi` | 0.315 0.050 200 | `#06393c` | plate gradient, top-left (lit 120°) |
| `brand.ink-rim` | 0.420 0.050 200 | `#285558` | 1-unit inner rim (dark taskbars) |
| `brand.ivory` | 0.970 0.012 178 | `#edf8f5` | "you" half on dark |
| `brand.lagoon-bright` | 0.820 0.140 178 | `#37e1c4` | "them" half on dark, i-dot |
| `brand.lagoon-deep` | 0.500 0.092 178 | `#007463` | "them" half on light |
| `brand.ink-text` | 0.240 0.035 205 | `#062427` | "you" half / wordmark on light |

### UI semantic tokens (12-step ramps in `tokens/primitives.tokens.json`)

| Slot | Light OKLCH · hex | Dark OKLCH · hex |
|---|---|---|
| bg.canvas | 0.985 0.006 178 · `#f6fbfa` | 0.170 0.011 178 · `#0a110f` (AMOLED `#000000`) |
| bg.surface | 1 0 · `#ffffff` | 0.215 0.011 178 · `#141b19` |
| bg.subtle | 0.955 0.011 178 · `#e9f3f0` | 0.260 0.011 178 · `#1e2624` |
| fg.default | 0.200 0.011 178 · `#111816` | 0.960 0.006 178 · `#eef3f2` |
| fg.muted | 0.470 0.011 178 · `#545d5b` | 0.740 0.011 178 · `#a4adab` |
| border.default | 0.860 0.011 178 · `#cad3d1` | 0.360 0.011 178 · `#373f3d` |
| accent.solid / border.focus | 0.520 0.096 178 · `#007b69` | 0.760 0.140 178 · `#00cdb1` |
| accent.solid-hover | 0.480 0.088 178 · `#006e5e` | 0.800 0.130 178 · `#43d9be` |
| accent.text | 0.450 0.083 178 · `#006455` | 0.840 0.110 178 · `#71e2ca` |
| accent.subtle | 0.950 0.030 178 · `#daf6ee` | 0.230 0.030 178 · `#0b221d` |
| fg.on-accent | 0.990 0 · `#fcfcfc` | 0.180 0.011 178 · `#0c1312` |

The semantic values equal what `packages/ui` `derivePalette()` produces for `{ hue: 178, chroma: 0.14 }`
(same lightness model, same tint = chroma × 0.08), so the brand pack and the live theme engine agree.
Light accent chroma is gamut-limited to 0.096 at L 0.52 for this hue — that is sRGB, not a choice.

### Contrast proof — `node brand/scripts/contrast-gate.mjs brand/contrast-pairs.json` (2026-10-04)

**65 pairs: 0 WCAG failures, 47 APCA-clean, 18 APCA advisories.** Selected rows (full list: run the
gate):

| Pair | WCAG | APCA Lc |
|---|---|---|
| light fg.default on bg.canvas | 17.24 | 101.7 |
| light fg.muted on bg.canvas | 6.50 | 80.4 |
| light accent.text on bg.canvas | 6.80 | 81.1 |
| light fg.on-accent on accent.solid | 5.07 | −78.6 |
| light border.focus on bg.surface (ui) | 5.20 | 75.2 |
| dark fg.default on bg.canvas | 17.03 | −98.8 |
| dark fg.muted on bg.canvas | 8.31 | −56.4 ⚠ |
| dark accent.text on bg.canvas | 12.21 | −77.1 |
| dark fg.on-accent on accent.solid | 9.28 | 64.5 ⚠ |
| AMOLED fg.default on black | 18.74 | −99.3 |
| ui-preset lagoon light: accent-fg on accent | 4.71 | −76.4 |
| ui-preset lagoon light: accent on bg (link text) | 4.63 | 69.9 ⚠ |
| icon: lagoon half on ink tile | 10.03 | −72.5 |
| icon: ink tile on Windows light taskbar | 14.91 | 96.1 |
| tray light: lagoon-deep on #f3f3f3 | 5.15 | 70.8 |
| OG: ivory tagline on ink | 15.24 | −99.5 |

APCA advisories and what they mean: dark `fg.muted` (Lc 54–56) is fine for secondary text ≥ 14 px /
400 but not for body copy — keep body in `fg.default`. Dark `fg.on-accent` on the solid accent
(Lc 64.5) needs button labels at ≥ 14 px / 600 (APCA "large" 60 passes). Light preset `accent on bg`
(Lc 69.9, WCAG 4.63) — use `accent.text` (step 11, Lc 81) for links instead of the solid step.

## Typography

| Role | Family | Licence | Files (self-hosted) | RO glyph check |
|---|---|---|---|---|
| UI, wordmark reference | **Inter Variable** (wght 100–900) | OFL-1.1 (`fonts/OFL-Inter.txt`) | `fonts/Inter-latin.woff2` 48 KB + `Inter-latin-ext.woff2` 85 KB | union **OK** (950 cp) |
| IDs, one-time codes, logs | **JetBrains Mono Variable** (wght 100–800) | OFL-1.1 (`fonts/OFL-JetBrainsMono.txt`) | `fonts/JetBrainsMono-latin.woff2` 40 KB + `-latin-ext.woff2` 15 KB | union **OK** (405 cp) |

Proof (2026-10-04, `check-glyphs.py`, fonttools in `.copilot-tmp/brand-venv`): the `latin` subset
alone is **missing ș ț Ș Ț ă Ă** and `latin-ext` alone is missing **â Â î Î** — so **both**
`@font-face` rules with their `unicode-range` must ship (they are in `dist/css/brand.css`). The RO OG
image is rendered with both subsets loaded (asserted by `render.mjs`). Inter is already the
`--font-sans` of `packages/ui/src/theme.css`; keep it. The wordmark is lettering, not Inter — no
licence question for the mark.

Scale (UI, minor third 1.2): 12 · 14 · 16 (body) · 19 · 23 · 28 · 33; display for marketing 1.25.
OpenType: `tnum` for IDs, timers and bitrate numbers; JetBrains Mono `zero` (slashed) for codes.

## Iconography

UI icons stay lucide-react (2 px stroke, round caps) — the mark is deliberately square-ended so it
never looks like "just another lucide icon". Tray states differ by **shape**: idle = symbol; active
session = symbol + accent dot bottom-right (`dist/tray/*-active-*`), never colour alone.

## Motion

Single source: `tokens/motion.tokens.json → logo.$extensions["app.scrin.logo"]` → generated
`dist/motion/logo.css` (CSS keyframes), `dist/motion/logo-motion.ts` (Motion 14 `motion/react`
variants), `dist/motion/logo-intro.svg` + `logo-thinking.svg` (self-contained animated SVG, no
Lottie), `android/drawable/avd_scrin_splash.xml` (AVD). Logo curve `logo.ease` =
`cubic-bezier(0.16, 1, 0.3, 1)` ("calm arrival"). Live demo + storyboard: `sheet/motion-demo.html`,
`sheet/motion-storyboard.png`.

| State | Meaning | Keyframes (24-unit grid) | Duration | Loop |
|---|---|---|---|---|
| intro | you + them arrive and draw into one s | ivory: dashoffset 1→0, translate (−2,−2)→0; lagoon: same from (+2,+2), +160 ms | 560 + 160 = 720 ms | once |
| thinking | data flowing both ways | both: dash 0.35, dashoffset 0→−1 linear; lagoon −½ period | 1200 ms period | 4 × (4.8 s < 5 s, WCAG 2.2.2) then static + text |
| success | halves lock | group scale 1 → 1.08 (40 %) → 1 | 320 ms | once |
| error | "no" head-shake, halves part | group x 0,−1,1,−0.6,0; halves ±0.6 apart at 50 % | 180 ms | once |
| hover | halves lean apart | ±0.35 diagonal (≤ 2 px at 128 px) | 140 ms | — |
| press | | scale 0.94 | 120 ms | — |
| morph | splash → title bar | `view-transition-name: scrin-mark` / Motion `layoutId="scrin-mark"`, `easing.move` | 380 ms | once |

Verified 2026-10-04 in Chromium (pixel diff vs a static, unanimated mark at 120 px):
intro-end **0 px**, success-end **0**, error-end **0**, live intro settled **0**; mid-frames differ
(intro-start 2118, thinking 1432) as intended. With `prefers-reduced-motion: reduce` every state —
including intro frame 0 — is **0 px** different from the static mark; thinking becomes a 2-step
opacity pulse (no movement). Rule for implementers: never put a transform **attribute** on
`.scrin-mark`; wrap it (the CSS transform of success/error would replace it).

## App icons

The tile is the ink plate (Fluent: lit top-left, 2-stop gradient at 135°, 1/24 inset, radius 5.25/24,
1-unit rim at 90 % for dark taskbars) with the symbol at 86 % (small) / 80 % (master). Android
adaptive: vector foreground with the s 44 dp tall inside the 66 dp safe circle (diagonal 55 dp),
gradient background, monochrome with the seam gap. Every size and format is listed in `ROLLOUT.md`.

### Legibility proof — `node brand/scripts/legibility.mjs` (2026-10-04)

Rendered at true size, measured on the real background. "runs" = separate strokes crossed by the
centre column inside the plate; the s must keep **3** (both counters open).

| Case | 16 px | 24 px | 32 px |
|---|---|---|---|
| A tile on Windows dark #202020 | 3 runs, 9.49:1 | 3, 9.48:1 | 3, 9.49:1 |
| A tile on Windows light #f3f3f3 | 3, 9.49:1 | 3, 12.55:1 | 3, 13.35:1 |
| A tray, dark taskbar | 3, 9.88:1 | 3, 9.88:1 | 3, 9.88:1 |
| A tray, light taskbar | 3, 5.15:1 | 3, 5.15:1 | 3, 5.15:1 |
| A one-colour black on white | 3, 21:1 | 3, 20.87:1 | 3, 21:1 |
| B twin panes (info) | 2 | 2 | 2 |
| C corner brackets (info) | 2 | 1 | 1 |
| D screen + code (info) | 3 (4.89:1 — dots smear) | 3 | 3 |

**RESULT: PASS** — exit 0. Lockup and wordmark: all shapes inside their viewBox.

## Concepts & decision (Gate B)

Comparison sheet: **`brand/sheet/comparison.html`** (live) / `brand/sheet/comparison.png` — each
concept at 16 · 24 · 32 · 48 · 128 · 512 px on dark and light, inside the Android circle mask with the
66 dp safe zone, in a Windows taskbar strip at 24 px and 36 px beside 8 real icons extracted from
this machine, one-colour, inverse, squint blur and true-pixel ×5 renders.

Directions generated (cliché scan: none use sparkle, orb, nodes, hexagon, infinity, purple→blue):

1. **Split S** (letter-as-object + architecture "two → one") — **chosen**
2. **Twin panes** — two overlapping screens (interface primitive)
3. **Corner brackets** — screen-capture frame + cursor block (product gesture)
4. **Screen + code dots** — monitor with one-time-code dots (category default)
5. Emoji-SAS face grid — rejected: emoji in a mark dates fast, illegible < 32 px
6. Comma-below accent (ș) — rejected: the name has no diacritic; feels like a flag pin
7. Handshake arrows ⇄ — rejected: generic sync icon, collides with every "transfer" glyph
8. Pure wordmark + pixel favicon — rejected: no symbol for tray/launcher contexts

**Why A wins.** It is the only one that is the initial *and* the product idea (two sides becoming one
connection) *and* survives 16 px: three bars and two open counters at every size (legibility table),
the micro cut is pixel-snapped, the one-colour version keeps the idea through the seam gap, and its
180° symmetry makes the motion set fall out of the construction (arrive, chase, lock, part). It owns a
letter, so it cannot be mistaken for a generic "remote desktop" glyph.

**Runners-up.** *B Twin panes* — clear and friendly, but it is what Parsec/Chrome Remote Desktop-style
icons already say, and at 16 px the overlap fills in (2 runs: the counter between panes closes).
*C Corner brackets* — distinctive and calm, but reads as "screenshot/crop tool" first (Snipping Tool,
ShareX); at 24–32 px it collapses to one run through the centre. *D Screen + code* — most literal and
explains the product, but it is the category default (TeamViewer/AnyDesk-era monitor) and the dots
smear to 4.89:1 at 16 px.

## Templates

`dist/og/og-{en,ro}.{png,webp}` (1200 × 630, lockup + tagline + ghosted master symbol; RO renders
ș ț ă â î with Inter latin-ext). TV banner 320 × 180 (xhdpi) and 640 × 360 (xxxhdpi). README header
→ use `logo/lockup-horizontal-on-{dark,light}.svg` with `<picture>` + `prefers-color-scheme`.

## Governance

```powershell
node brand/scripts/build.mjs        # tokens, contrast pairs, SVG masters, Android XML, motion outputs
node brand/scripts/sheet.mjs        # comparison sheet + motion demo HTML
node brand/scripts/render.mjs       # PNG / WebP / ICO exports + sheet screenshots (needs apps/web Playwright)
node brand/scripts/contrast-gate.mjs brand/contrast-pairs.json      # exit 1 on any WCAG failure
<venv>\Scripts\python.exe brand/scripts/check-glyphs.py --union brand/fonts/Inter-latin.woff2 brand/fonts/Inter-latin-ext.woff2
node brand/scripts/legibility.mjs   # exit 1 if the mark loses a counter or < 3:1 at 16/24/32
```

SemVer: major = mark or primary colour change · minor = new asset/token · patch = fix. A WCAG failure
is fixed in the token, never in the gate. CI drift check (to add when the brand pack is wired into
CI): run build.mjs and `git diff --exit-code brand/`.

## Roll-out

See `ROLLOUT.md` — every target file with source asset, size and format. This pack did not edit
anything outside `brand/`.

## Changelog

### 0.1.0 — 2026-10-04

- First identity: split-S mark (3 optical sizes), lettered wordmark, lockups, lagoon (hue 178) on ink.
- DTCG 2025.10 tokens (12-step ramps light/dark, AMOLED), 65 contrast pairs (0 WCAG failures).
- Inter + JetBrains Mono self-hosted, Romanian glyph union verified.
- Exports: web favicon set + PWA, Windows ICO / Tauri / MSIX (42 files), tray light/dark/active,
  Android adaptive (fg/bg/monochrome) + legacy WebP + Play 512 + splash + AVD, Android TV banner, OG EN/RO.
- Logo motion set (intro, thinking, success, error, hover, press, morph) from one source.
- Known limits: concept choice and gate answers made by the agent under delegation (user was not
  reachable mid-run); macOS/iOS icons out of scope (not v1 platforms); design-critic skill not
  installed on this machine — scoring replaced by the numeric legibility + contrast gates.
