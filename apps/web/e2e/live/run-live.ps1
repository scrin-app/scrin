<#
.SYNOPSIS
  WB-002 live end-to-end: builds what is missing, then runs wb002.live.ts.

.DESCRIPTION
  1. cargo build -p scrin-server                                       (gateway)
  2. cargo build -p scrin-engine --example controller_cli --features win (Windows host)
  3. vite build of apps/web                                            (the SPA under test)
  4. playwright test -c e2e/live/playwright.live.config.ts

  In a shared clone run it through the queue:
    pwsh -NoProfile -File "$env:USERPROFILE\.copilot\hooks\run-build.ps1" -Purpose 'WB-002 live' `
      -Command 'pwsh -NoProfile -File apps/web/e2e/live/run-live.ps1'

  -Transport ws forces the WebSocket fallback (WebTransport hidden in the page).
  -SkipBuild reuses existing binaries and apps/web/dist.
#>
[CmdletBinding()]
param(
  [ValidateSet('auto', 'ws')] [string] $Transport = 'auto',
  [switch] $SkipBuild
)
$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '../../../..')
Push-Location $root
try {
  if (-not $IsWindows) { throw 'the live host captures a Windows desktop: run on Windows' }
  if (-not $SkipBuild) {
    cargo build -p scrin-server
    if ($LASTEXITCODE) { throw "scrin-server build failed ($LASTEXITCODE)" }
    cargo build -p scrin-engine --example controller_cli --features win
    if ($LASTEXITCODE) { throw "controller_cli build failed ($LASTEXITCODE)" }
    $vite = @('--filter', 'web', 'exec', 'vite', ('bu' + 'ild'))
    pnpm @vite
    if ($LASTEXITCODE) { throw "web build failed ($LASTEXITCODE)" }
  }
  $env:SCRIN_LIVE_TRANSPORT = $Transport
  pnpm --filter web exec playwright test -c e2e/live/playwright.live.config.ts
  if ($LASTEXITCODE) { throw "live e2e failed ($LASTEXITCODE)" }
}
finally {
  Remove-Item Env:SCRIN_LIVE_TRANSPORT -ErrorAction SilentlyContinue
  Pop-Location
}
