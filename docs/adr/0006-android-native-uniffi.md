# ADR-0006: Android — native Kotlin/Compose with the Rust core over UniFFI

Date: 2026-10-04 · Status: Accepted · Tracker: D08, D11, D18, D22

## Context

v1 needs an Android client (to Windows hosts) and an Android attended host, plus an Android TV
client (D19). Android hosting is constrained by the platform: since Android 14 a MediaProjection
token is single use and the user must consent for every capture session; input injection is only
possible through an AccessibilityService (or as Device Owner); Android 16 Advanced Protection
mode blocks non-accessibility-tool apps from using the accessibility API. The protocol and crypto
must be the same code as desktop (ADR-0002). Test device: Samsung A51 (D22).

## Decision

- **UI**: Kotlin 2.4, Jetpack Compose (BOM 2026.09), Material 3, navigation3,
  kotlinx-serialization, DataStore. Theme generated from the shared design tokens (ADR-0010).
  Modules: `android/app`, `android/tv`, `android/core-ffi`.
- **Core**: `crates/scrin-ffi` exposes the session, net and crypto API via **UniFFI in library
  mode**; built with `cargo-ndk` for arm64-v8a, armeabi-v7a, x86_64. Host-side unit tests load the
  host-built library through **JNA** so Kotlin tests run on the JVM without an emulator (titi pattern).
- **Client**: MediaCodec low-latency decode → SurfaceView; touch → relative/absolute mouse
  modes, on-screen keyboard with scancode mapping.
- **Host is attended only** (D08): MediaProjection consent every session (foreground service
  type `mediaProjection`), audio via AudioPlaybackCapture where allowed.
- **Input on the host**: AccessibilityService gestures + global actions, with a **prominent
  disclosure** screen before enabling (Play policy). Device Owner provisioning is an optional path
  for unattended kiosks/fleets.
- **Advanced Protection mode** or a missing accessibility grant → automatic **view-only** fallback
  with an explanation, never a silent failure.
- **Distribution** (D18): Google Play, GitHub Releases (APK), and a `foss` flavour for F-Droid
  with no proprietary dependencies (no Play Services, no FCM).

## Consequences

- Native feel and direct access to MediaCodec/SurfaceView/MediaProjection with no bridge layer.
- Kotlin UI is a second UI codebase next to `packages/ui`; tokens are shared, components are not.
- Unattended Android hosting is impossible without Device Owner — documented as a platform limit.
- Play review of the AccessibilityService use is a release risk; the disclosure and a policy
  declaration are required before the first Play upload.
- UniFFI generated bindings are checked by a drift test so Kotlin and Rust never disagree.

## Alternatives considered

- **Tauri mobile** — reuses the React UI, but video would again be inside a WebView and
  MediaProjection/Accessibility need native plugins anyway. Rejected.
- **Flutter** — good UI, but Dart ↔ Rust bridge plus platform channels for every native API, and no
  reuse with the web UI. Rejected.
- **React Native / Expo** — legacy in this stack; same native-module burden for media and input.
  Rejected.
- **Kotlin Multiplatform for the core** — would duplicate protocol/crypto already written in Rust
  for desktop and web. Rejected.
