# Clippy for the Android target. Host clippy never compiles #[cfg(target_os = "android")]
# code, so this lane catches it. Needs ANDROID_HOME (or the default SDK path) with an NDK.
param([string] $Target = 'aarch64-linux-android', [int] $Api = 26)
$ErrorActionPreference = 'Stop'
$sdk = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } elseif ($env:ANDROID_SDK_ROOT) { $env:ANDROID_SDK_ROOT } else { Join-Path $env:LOCALAPPDATA 'Android\Sdk' }
$ndkRoot = Join-Path $sdk 'ndk'
if (-not (Test-Path $ndkRoot)) { Write-Host "SKIP: no NDK under $ndkRoot"; exit 0 }
$ndk = Get-ChildItem $ndkRoot -Directory | Sort-Object { [version]($_.Name -replace '[^\d.]', '') } | Select-Object -Last 1
$hostTag = if ($IsWindows) { 'windows-x86_64' } elseif ($IsMacOS) { 'darwin-x86_64' } else { 'linux-x86_64' }
$bin = Join-Path $ndk.FullName "toolchains/llvm/prebuilt/$hostTag/bin"
$ext = if ($IsWindows) { '.cmd' } else { '' }
$triple = $Target -replace '-', '_'
Set-Item "env:CC_$triple" (Join-Path $bin "$Target$Api-clang$ext")
Set-Item "env:AR_$triple" (Join-Path $bin "llvm-ar$(if ($IsWindows) { '.exe' })")
Write-Host "NDK $($ndk.Name) -> $Target"
# Desktop-only crates (Tauri shell, Windows service) are not built for Android.
cargo clippy --workspace --exclude scrin-desktop --exclude scrin-service --target $Target -- -D warnings
exit $LASTEXITCODE
