<#
.SYNOPSIS
  Builds crates/scrin-wasm for the browser and regenerates packages/protocol/wasm/.

.DESCRIPTION
  cargo (wasm32-unknown-unknown, release, opt-level "z") -> wasm-bindgen --target web
  (version must equal the wasm-bindgen crate in Cargo.lock) -> wasm-opt -Oz when installed.
  The generated files are committed so the JS lanes do not need a Rust toolchain;
  `packages/protocol/src/wasm.test.ts` replays testvectors/gateway-session.json against them.
#>
[CmdletBinding()]
param([switch] $NoOpt)
$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '../../..')
$out = Join-Path $root 'packages/protocol/wasm'
Push-Location $root
try {
  $env:CARGO_PROFILE_RELEASE_OPT_LEVEL = 'z'
  cargo build -p scrin-wasm --target wasm32-unknown-unknown --release
  if ($LASTEXITCODE) { throw "cargo failed ($LASTEXITCODE)" }
  $wasm = Join-Path $root 'target/wasm32-unknown-unknown/release/scrin_wasm.wasm'
  wasm-bindgen $wasm --target web --out-dir $out --out-name scrin_wasm `
    --remove-name-section --remove-producers-section --omit-default-module-path
  if ($LASTEXITCODE) { throw "wasm-bindgen failed ($LASTEXITCODE)" }
  $bg = Join-Path $out 'scrin_wasm_bg.wasm'
  if (-not $NoOpt -and (Get-Command wasm-opt -ErrorAction SilentlyContinue)) {
    wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext `
      --enable-mutable-globals --enable-reference-types --enable-multivalue -o $bg $bg
    if ($LASTEXITCODE) { throw "wasm-opt failed ($LASTEXITCODE)" }
  }
  else { Write-Host 'wasm-opt not found: skipped (install binaryen for a smaller module)' }
  Remove-Item (Join-Path $out '.gitignore') -ErrorAction SilentlyContinue
  $bytes = [IO.File]::ReadAllBytes($bg)
  $ms = [IO.MemoryStream]::new()
  $gz = [IO.Compression.GZipStream]::new($ms, [IO.Compression.CompressionLevel]::SmallestSize)
  $gz.Write($bytes, 0, $bytes.Length); $gz.Dispose()
  '{0}: {1:N0} B raw, {2:N0} B gzip' -f 'scrin_wasm_bg.wasm', $bytes.Length, $ms.ToArray().Length
}
finally {
  Remove-Item Env:CARGO_PROFILE_RELEASE_OPT_LEVEL -ErrorAction SilentlyContinue
  Pop-Location
}
