<#
.SYNOPSIS
  Every quality gate, in parallel lanes, with per-lane logs and a summary table.

.DESCRIPTION
  Lanes (each runs only when its inputs exist; a missing optional tool is a
  loud SKIP, never a silent pass):
    rust       cargo fmt --check, clippy -D warnings, cargo test, cargo deny
    js         pnpm typecheck / lint / test (only scripts the root package.json defines)
    proto      buf lint + buf breaking against main (needs `buf` on PATH)
    android    gradlew lint + unit tests (needs android/gradlew)
    size       scripts/size-budgets.ps1 (when it exists)
    e2e        pnpm e2e (opt-in: not part of the default set; run -Only e2e)
    invariants check-invariants.ps1, test-invariants.ps1 (mutation), check-tracker.ps1
    security   gitleaks over git history, pnpm audit --audit-level high

  Steps inside a lane are serial and stop at the first failure (later steps
  depend on earlier ones); lanes run in parallel thread jobs.
  Logs: .copilot-tmp/gates/<stamp>/<lane>.log. Exit 1 on any FAIL.

.EXAMPLE
  pwsh -NoProfile -File scripts/gates.ps1
  pwsh -NoProfile -File scripts/gates.ps1 -Only rust,invariants
#>
[CmdletBinding()]
param(
  # Comma-separated or array. `pwsh -File gates.ps1 -Only a,b` binds ONE string, so split here.
  [string[]] $Only = @('all')
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$allLanes = @('rust', 'js', 'proto', 'android', 'size', 'e2e', 'invariants', 'security')
$defaultLanes = @($allLanes | Where-Object { $_ -ne 'e2e' })
$requested = @($Only | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim().ToLowerInvariant() } | Where-Object { $_ })
$unknown = @($requested | Where-Object { $_ -ne 'all' -and $_ -notin $allLanes })
if ($unknown) { Write-Host "unknown lane(s): $($unknown -join ', '); valid: $($allLanes -join ', '), all" -ForegroundColor Red; exit 2 }
$selected = if ($requested -contains 'all') { $defaultLanes } else { @($allLanes | Where-Object { $_ -in $requested }) }

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$logDir = Join-Path $root ".copilot-tmp/gates/$stamp"
New-Item -ItemType Directory -Force $logDir | Out-Null

function Has([string] $cmd) { [bool](Get-Command $cmd -ErrorAction SilentlyContinue) }
function Step([string] $name, [string] $cmd, [string] $dir = '.', [string] $skip = '') {
  @{ Name = $name; Cmd = $cmd; Dir = $dir; Skip = $skip }
}
$rootPkg = $null
if (Test-Path 'package.json') {
  try { $rootPkg = Get-Content 'package.json' -Raw | ConvertFrom-Json } catch { $rootPkg = $null }
}
function PnpmScript([string] $script) {
  if (-not $rootPkg) { return Step "pnpm $script" '' '.' 'package.json unreadable' }
  if (-not ($rootPkg.scripts.PSObject.Properties.Name -contains $script)) {
    return Step "pnpm $script" '' '.' "no '$script' script in package.json"
  }
  Step "pnpm $script" "pnpm run $script"
}

# ---- lane plans: @{ Skip = '<reason>' } skips the whole lane -------------
$plan = [ordered]@{}

$plan.rust = if (-not (Test-Path 'Cargo.toml')) { @{ Skip = 'no Cargo.toml' } }
elseif (-not (Has 'cargo')) { @{ Skip = 'cargo not on PATH' } }
else {
  @{ Steps = @(
      Step 'cargo fmt' 'cargo fmt --all --check'
      Step 'cargo clippy' 'cargo clippy --workspace --all-targets -- -D warnings'
      Step 'cargo test' 'cargo test --workspace'
      Step 'cargo deny' 'cargo deny check' '.' $(if (Has 'cargo-deny') { '' } else { 'cargo-deny not installed (cargo install --locked cargo-deny)' })
    ) }
}

$plan.js = if (-not (Test-Path 'package.json')) { @{ Skip = 'no package.json' } }
elseif (-not (Has 'pnpm')) { @{ Skip = 'pnpm not on PATH' } }
else { @{ Steps = @((PnpmScript 'typecheck'), (PnpmScript 'lint'), (PnpmScript 'test')) } }

$protoFiles = @(if (Test-Path 'proto') { rg --files proto -g '*.proto' })
$plan.proto = if (-not $protoFiles) { @{ Skip = 'no proto/**/*.proto' } }
elseif (-not (Has 'buf')) { @{ Skip = 'buf NOT INSTALLED - proto not checked (winget install bufbuild.buf)' } }
else {
  git rev-parse --verify -q 'main:proto' 2>$null | Out-Null
  $hasBase = $LASTEXITCODE -eq 0
  @{ Steps = @(
      Step 'buf lint' 'buf lint'
      Step 'buf breaking' "buf breaking --against '.git#branch=main'" '.' $(if ($hasBase) { '' } else { 'main has no proto/ yet (nothing to compare)' })
    ) }
}

$gradlew = if ($IsWindows) { 'android/gradlew.bat' } else { 'android/gradlew' }
$plan.android = if (-not (Test-Path 'android')) { @{ Skip = 'no android/' } }
elseif (-not (Test-Path $gradlew)) { @{ Skip = "no $gradlew" } }
else {
  $g = if ($IsWindows) { '.\gradlew.bat' } else { './gradlew' }
  @{ Steps = @(Step 'gradle lint+test' "$g --console=plain lint testDebugUnitTest" 'android') }
}

$plan.size = if (Test-Path 'scripts/size-budgets.ps1') {
  @{ Steps = @(Step 'size budgets' 'pwsh -NoProfile -File scripts/size-budgets.ps1') }
} else { @{ Skip = 'scripts/size-budgets.ps1 not written yet (F-011)' } }

$plan.e2e = if (-not $rootPkg -or -not ($rootPkg.scripts.PSObject.Properties.Name -contains 'e2e')) { @{ Skip = "no 'e2e' script in package.json" } }
else { @{ Steps = @(Step 'pnpm e2e' 'pnpm run e2e') } }

$plan.invariants = @{ Steps = @(
    Step 'check-invariants' 'pwsh -NoProfile -File scripts/check-invariants.ps1'
    Step 'check-tracker' 'pwsh -NoProfile -File scripts/check-tracker.ps1'
    Step 'test-invariants' 'pwsh -NoProfile -File scripts/test-invariants.ps1'
  ) }

git rev-parse --verify -q HEAD 2>$null | Out-Null
$hasHead = $LASTEXITCODE -eq 0
$plan.security = @{ Steps = @(
    Step 'gitleaks' 'gitleaks git --no-banner --redact --exit-code 1 --config .gitleaks.toml .' '.' $(
      if (-not (Has 'gitleaks')) { 'gitleaks NOT INSTALLED (winget install gitleaks.gitleaks)' } elseif (-not $hasHead) { 'no commits yet' } else { '' })
    Step 'pnpm audit' 'pnpm audit --audit-level high' '.' $(
      if (-not (Test-Path 'pnpm-lock.yaml')) { 'no pnpm-lock.yaml' } elseif (-not (Has 'pnpm')) { 'pnpm not on PATH' } else { '' })
  ) }

# ---- run ----------------------------------------------------------------
$total = [Diagnostics.Stopwatch]::StartNew()
$results = [System.Collections.Generic.List[object]]::new()
$jobs = @()
foreach ($lane in $selected) {
  $p = $plan[$lane]
  $log = Join-Path $logDir "$lane.log"
  if ($p.Skip) {
    "SKIPPED: $($p.Skip)" | Set-Content $log
    $results.Add([pscustomobject]@{ Lane = $lane; Status = 'SKIP'; Seconds = 0.0; Detail = $p.Skip; Log = $log })
    continue
  }
  $jobs += Start-ThreadJob -Name $lane -ArgumentList $root, $lane, $log, $p.Steps -ScriptBlock {
    param($root, $lane, $log, $steps)
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $status = 'PASS'; $detail = [System.Collections.Generic.List[string]]::new()
    "# lane $lane  $(Get-Date -Format o)" | Set-Content $log
    foreach ($s in $steps) {
      if ($s.Skip) {
        "`n## $($s.Name): SKIPPED - $($s.Skip)" | Add-Content $log
        $detail.Add("$($s.Name) skipped: $($s.Skip)")
        continue
      }
      "`n## $($s.Name)`n> $($s.Cmd)" | Add-Content $log
      $st = [Diagnostics.Stopwatch]::StartNew()
      & pwsh -NoProfile -WorkingDirectory (Join-Path $root $s.Dir) -Command $s.Cmd *>> $log
      $code = $LASTEXITCODE
      "## $($s.Name): exit $code in $([math]::Round($st.Elapsed.TotalSeconds, 1)) s" | Add-Content $log
      if ($code -ne 0) { $status = 'FAIL'; $detail.Add("$($s.Name) exit $code"); break }
    }
    [pscustomobject]@{ Lane = $lane; Status = $status; Seconds = [math]::Round($sw.Elapsed.TotalSeconds, 1); Detail = ($detail -join '; '); Log = $log }
  }
}
if ($jobs) { foreach ($r in ($jobs | Receive-Job -Wait -AutoRemoveJob)) { $results.Add($r) } }
$total.Stop()

$order = @{ FAIL = 0; PASS = 1; SKIP = 2 }
$results | Sort-Object { $order[$_.Status] }, Lane | Format-Table Lane, Status, Seconds, Detail -AutoSize -Wrap | Out-String -Width 200 | Write-Host
'wall: {0:N1}s  logs: {1}' -f $total.Elapsed.TotalSeconds, $logDir | Write-Host
foreach ($r in $results) {
  if ($r.Status -eq 'SKIP' -or $r.Detail -match 'skipped') { Write-Host "  SKIPPED in $($r.Lane): $($r.Detail)" -ForegroundColor Yellow }
}
$failed = @($results | Where-Object Status -EQ 'FAIL')
if ($failed) {
  Write-Host ('FAILED: ' + ($failed.Lane -join ', ') + '  (see the lane logs above)') -ForegroundColor Red
  exit 1
}
Write-Host 'no lane failed' -ForegroundColor Green
exit 0
