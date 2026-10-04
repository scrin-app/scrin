<#
.SYNOPSIS
  Size budgets (F-011): every shipped artifact vs size-budget.json.

.DESCRIPTION
  Measures what is already built - it never builds. An artifact that is not
  built is a per-row SKIP with the reason (a SKIP is not a PASS).
    web       apps/web/dist via apps/web/scripts/check-size.mjs (gzip, initial JS/CSS, route chunk)
    wasm      packages/protocol/wasm/scrin_wasm_bg.wasm (raw + gzip -9)
    desktop   target/release/scrin-desktop(.exe) + NSIS/MSI installers in target/release/bundle
    server    target/release/scrin-server(.exe)
    android   release APKs (per ABI when split) and AABs under android/*/build/outputs
  Prints actual vs budget vs delta; exit 1 when any measured artifact is over budget.

.EXAMPLE
  pwsh -NoProfile -File scripts/size-budgets.ps1
#>
[CmdletBinding()]
param(
  # Alternate budget file (used to prove the over-budget path fails).
  [string] $BudgetFile = ''
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
[Threading.Thread]::CurrentThread.CurrentCulture = [Globalization.CultureInfo]::InvariantCulture

if (-not $BudgetFile) { $BudgetFile = Join-Path $root 'size-budget.json' }
$budget = Get-Content $BudgetFile -Raw | ConvertFrom-Json
$rows = [System.Collections.Generic.List[object]]::new()

function Add-Row([string] $name, [Nullable[long]] $actual, [long] $limit, [string] $note = '') {
  $status = if ($null -eq $actual) { 'SKIP' } elseif ($actual -le $limit) { 'PASS' } else { 'FAIL' }
  $rows.Add([pscustomobject]@{ Artifact = $name; Actual = $actual; Budget = $limit; Status = $status; Note = $note })
}
function Get-GzipLength([string] $path) {
  $bytes = [IO.File]::ReadAllBytes($path)
  $ms = [IO.MemoryStream]::new()
  $gz = [IO.Compression.GZipStream]::new($ms, [IO.Compression.CompressionLevel]::SmallestSize, $true)
  $gz.Write($bytes, 0, $bytes.Length)
  $gz.Dispose()
  $ms.Length
}
function Format-Size([Nullable[long]] $n) {
  if ($null -eq $n) { return '-' }
  if ([math]::Abs($n) -ge 1MB) { return '{0:N2} MB' -f ($n / 1MB) }
  '{0:N1} KB' -f ($n / 1KB)
}
function Get-Built([string[]] $patterns) {
  @(foreach ($p in $patterns) { Get-ChildItem -Path $p -File -ErrorAction SilentlyContinue })
}
$exe = if ($IsWindows) { '.exe' } else { '' }

# ---- web: reuse the existing measurement (apps/web/scripts/check-size.mjs) ----
$webNames = [ordered]@{
  'initial JS (gzip)'          = 'initialJsGzip'
  'initial CSS (gzip)'         = 'initialCssGzip'
  'largest route chunk (gzip)' = 'largestRouteChunkGzip'
}
if (-not (Test-Path 'apps/web/dist/index.html')) {
  foreach ($k in $webNames.Keys) { Add-Row "web $k" $null $budget.web.($webNames[$k]) 'apps/web/dist not built (pnpm --filter web build)' }
} else {
  $out = @(node apps/web/scripts/check-size.mjs 2>&1 | ForEach-Object { "$_" })
  $seen = 0
  foreach ($line in $out) {
    if ($line -match '^(PASS|FAIL)\s+(.+?)\s{2,}\s*([\d.]+) KB / ([\d.]+) KB') {
      $key = $webNames[$Matches[2].Trim()]
      if (-not $key) { continue }
      Add-Row "web $($Matches[2].Trim())" ([long]([double]$Matches[3] * 1024)) $budget.web.$key 'check-size.mjs'
      $seen++
    }
  }
  if ($seen -ne $webNames.Count) {
    Write-Host ($out -join "`n")
    throw "check-size.mjs output not understood ($seen/$($webNames.Count) rows)"
  }
}

# ---- wasm (browser client core) ----
$wasm = 'packages/protocol/wasm/scrin_wasm_bg.wasm'
if (Test-Path $wasm) {
  Add-Row 'wasm scrin_wasm_bg.wasm (raw)' (Get-Item $wasm).Length $budget.wasm.rawBytes $wasm
  Add-Row 'wasm scrin_wasm_bg.wasm (gzip)' (Get-GzipLength $wasm) $budget.wasm.gzipBytes $wasm
} else {
  Add-Row 'wasm scrin_wasm_bg.wasm (raw)' $null $budget.wasm.rawBytes "$wasm not built (wasm-pack build crates/scrin-wasm)"
  Add-Row 'wasm scrin_wasm_bg.wasm (gzip)' $null $budget.wasm.gzipBytes "$wasm not built"
}

# ---- desktop ----
$desk = "target/release/scrin-desktop$exe"
if (Test-Path $desk) { Add-Row 'desktop exe (release)' (Get-Item $desk).Length $budget.desktop.exeBytes $desk }
else { Add-Row 'desktop exe (release)' $null $budget.desktop.exeBytes "$desk not built (cargo tauri build)" }
$installers = Get-Built @('target/release/bundle/nsis/*.exe', 'target/release/bundle/msi/*.msi')
if ($installers) {
  foreach ($i in $installers) { Add-Row "desktop installer $($i.Name)" $i.Length $budget.desktop.installerBytes $i.Directory.Name }
} else { Add-Row 'desktop installer (nsis/msi)' $null $budget.desktop.installerBytes 'target/release/bundle not built (cargo tauri build)' }

# ---- server ----
$srv = "target/release/scrin-server$exe"
if (Test-Path $srv) { Add-Row 'scrin-server (release)' (Get-Item $srv).Length $budget.server.binaryBytes $srv }
else { Add-Row 'scrin-server (release)' $null $budget.server.binaryBytes "$srv not built (cargo build --release -p scrin-server)" }

# ---- android: release only; one row per APK (per ABI when split) and per AAB ----
$apks = Get-Built @('android/*/build/outputs/apk/release/*.apk', 'android/*/build/outputs/apk/*/release/*.apk')
$aabs = Get-Built @('android/*/build/outputs/bundle/*[Rr]elease/*.aab')
foreach ($a in $apks) { Add-Row "android $($a.Name)" $a.Length $budget.android.apkBytes 'release apk' }
foreach ($a in $aabs) { Add-Row "android $($a.Name)" $a.Length $budget.android.aabBytes 'release aab' }
if (-not $apks) { Add-Row 'android release APK' $null $budget.android.apkBytes 'no release APK built (gradlew assembleRelease); debug APKs are not budgeted' }
if (-not $aabs) { Add-Row 'android release AAB' $null $budget.android.aabBytes 'no release AAB built (gradlew bundleRelease)' }

# ---- report ----
$table = foreach ($r in $rows) {
  $delta = if ($null -eq $r.Actual) { '-' } else {
    $d = $r.Actual - $r.Budget
    '{0}{1} ({2:N1}%)' -f $(if ($d -gt 0) { '+' } else { '' }), (Format-Size $d), (100.0 * $d / $r.Budget)
  }
  [pscustomobject]@{ Status = $r.Status; Artifact = $r.Artifact; Actual = Format-Size $r.Actual; Budget = Format-Size $r.Budget; Delta = $delta; Note = $r.Note }
}
$table | Format-Table -AutoSize -Wrap | Out-String -Width 220 | Write-Host
$fail = @($rows | Where-Object Status -EQ 'FAIL')
$skip = @($rows | Where-Object Status -EQ 'SKIP')
$pass = @($rows | Where-Object Status -EQ 'PASS')
Write-Host ('size: {0} pass, {1} fail, {2} skipped (not built - not checked)' -f $pass.Count, $fail.Count, $skip.Count)
if ($fail) {
  Write-Host ('OVER BUDGET: ' + ($fail.Artifact -join ', ')) -ForegroundColor Red
  exit 1
}
exit 0
