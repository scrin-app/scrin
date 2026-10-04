---
applyTo: 'android/**'
---

# Android (client, attended host, TV)

## Toolchain

- Kotlin 2.4.20, AGP 9.4, Compose BOM 2026.09, navigation3, kotlinx-serialization. Versions only in
  `android/gradle/libs.versions.toml` — never inline a version in a module `build.gradle.kts`.
- Application id `ro.dragoscatalin.scrin`. Product flavours `gms` (Play) and `foss` (F-Droid, GitHub):
  `foss` must not depend on Google Play Services, Firebase or any proprietary SDK.
- `versionName` / `versionCode` are written only by `scripts/version.ps1`
  (`versionCode = MAJOR*1_000_000 + MINOR*1_000 + PATCH`). Never edit them by hand.

## Rust core via UniFFI

- `android/core-ffi` consumes `crates/scrin-ffi` in UniFFI library mode: `cargo-ndk` builds the `.so`
  per ABI, bindings are generated from the built library, JNA loads it.
- Business logic stays in Rust. Kotlin owns UI, Android APIs (MediaProjection, MediaCodec,
  AccessibilityService, Keystore) and lifecycle. Do not reimplement protocol or crypto in Kotlin.
- A change to an exported `scrin-ffi` signature regenerates bindings in the same commit.
- Calls into Rust that may block run on `Dispatchers.IO`, never on the main thread.

## Attended host (D08)

- MediaProjection consent is requested **every session**; never cache or reuse a projection token.
- Screen capture runs in a foreground service with `foregroundServiceType="mediaProjection"` and a
  persistent notification with a Stop action.
- Remote input uses an `AccessibilityService` (gestures + global actions). Before enabling it, show a
  **prominent disclosure** screen (what is collected, why, how to turn it off) and require an explicit tap.
- **Never set `android:isAccessibilityTool="true"`** — scrin is not an accessibility tool and the
  declaration would be false. `scripts/check-invariants.ps1` fails on it.
- Unattended host is only possible with Device Owner provisioning (optional); do not add workarounds.

## UI

- Material 3 Compose with tokens generated from the shared theme; light/dark/system, dynamic accent.
- Every string in `res/values/strings.xml` **and** `res/values-ro/strings.xml`; no literals in
  composables. Content descriptions for icon-only controls; touch targets ≥ 48 dp.
- Respect `Settings.Global.ANIMATOR_DURATION_SCALE` = 0 (reduced motion).
- Edge-to-edge with proper insets; TV module supports D-pad focus on every control.

## Hygiene

- No `Log.d` of codes, keys, SAS or clipboard contents. Release builds strip debug logs (R8 rules).
- Video decode with MediaCodec low-latency into a `SurfaceView`; no `TextureView` on the hot path.
- Request runtime permissions at the point of use with a rationale; never at startup.

## Verify

```powershell
pwsh -NoProfile -File scripts/gates.ps1 -Only android
```

Then the `device-verify` skill on the Galaxy A51 (`adb -s R58N94BMLJY`). Host gates never exercise
MediaProjection, AccessibilityService or MediaCodec — those claims are EXPECTED until seen on device.
