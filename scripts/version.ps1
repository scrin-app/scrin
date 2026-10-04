<#
.SYNOPSIS
  Bump every shipped version in lockstep.

.DESCRIPTION
  scrin's version lives in several manifests that must agree:
    Cargo.toml                       [workspace.package] version
    package.json, apps/*/package.json, packages/*/package.json   "version"
    apps/desktop/src-tauri/tauri.conf.json  "version"            (when present)
    android/gradle.properties        scrin.versionName / scrin.versionCode (when present)

  versionCode is derived, not chosen: MAJOR*1_000_000 + MINOR*1_000 + PATCH —
  monotonic in SemVer order. Android and the Tauri updater refuse downgrades,
  so the script refuses a version not above the current one unless -Force.
  Cargo.lock is refreshed with `cargo update --workspace --offline` unless -NoLock.

.EXAMPLE
  pwsh -NoProfile -File scripts/version.ps1 0.2.0 -DryRun
  pwsh -NoProfile -File scripts/version.ps1 0.2.0
#>
[CmdletBinding()]
param(
  [Parameter(Position = 0, Mandatory)]
  [ValidatePattern('^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$')]
  [string] $Version,
  [switch] $DryRun,
  [switch] $NoLock,
  [switch] $Force
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function VersionCode([string] $v) {
  $p = (($v -split '-')[0] -split '\.') | ForEach-Object { [int]$_ }
  foreach ($n in $p) { if ($n -gt 999) { throw "component $n exceeds 999; versionCode would collide" } }
  $p[0] * 1000000 + $p[1] * 1000 + $p[2]
}
function ReadText([string] $p) { [IO.File]::ReadAllText((Join-Path $root $p)) }
function WriteText([string] $p, [string] $text) {
  if ($DryRun) { return }
  [IO.File]::WriteAllText((Join-Path $root $p), $text.Replace("`r`n", "`n"), [Text.UTF8Encoding]::new($false))
}
# One anchored replacement that must match exactly once; zero or two matches = layout change to look at.
function Edit([string] $path, [string] $pattern, [string] $replacement, [string] $label) {
  $text = ReadText $path
  $m = [regex]::Matches($text, $pattern, 'Multiline')
  if ($m.Count -ne 1) { throw "$path`: expected exactly one match for $label, found $($m.Count)" }
  $new = [regex]::Replace($text, $pattern, $replacement, 'Multiline')
  if ($new -eq $text) { "  = $path ($label already $Version)"; return }
  "  ~ $path  ($label)"
  WriteText $path $new
}

$wsPattern = '(?m)(^\[workspace\.package\]\s*\r?\nversion\s*=\s*")[^"]+(")'
$current = [regex]::Match((ReadText 'Cargo.toml'), $wsPattern).Groups[0].Value -replace '(?s).*version\s*=\s*"([^"]+)".*', '$1'
if (-not $current) { throw 'Cargo.toml: [workspace.package] must start with version = "x.y.z"' }
$code = VersionCode $Version
"version: $current -> $Version   versionCode: $(VersionCode $current) -> $code$(if ($DryRun) { '   (dry run)' })"
if ([version](($Version -split '-')[0]) -le [version](($current -split '-')[0]) -and -not $Force) {
  throw "$Version is not above $current; Android and the updater would refuse it. -Force only for an unshipped version."
}

Edit 'Cargo.toml' $wsPattern "`${1}$Version`${2}" 'workspace version'
$pkgs = @(rg --files -g 'package.json' -g '!**/node_modules/**' | ForEach-Object { $_ -replace '\\', '/' } | Where-Object { $_ -match '^(package\.json|apps/[^/]+/package\.json|packages/[^/]+/package\.json)$' })
foreach ($p in $pkgs) { Edit $p '(?m)^(  "version":\s*")[^"]+(")' "`${1}$Version`${2}" 'version' }
$tauri = 'apps/desktop/src-tauri/tauri.conf.json'
if (Test-Path $tauri) { Edit $tauri '(?m)^(  "version":\s*")[^"]+(")' "`${1}$Version`${2}" 'tauri version' }
$gp = 'android/gradle.properties'
if ((Test-Path $gp) -and (ReadText $gp) -match '(?m)^scrin\.versionName=') {
  Edit $gp '(?m)^(scrin\.versionName=).*$' "`${1}$Version" 'versionName'
  Edit $gp '(?m)^(scrin\.versionCode=).*$' "`${1}$code" 'versionCode'
}

if (-not $NoLock -and -not $DryRun) {
  cargo update --workspace --offline
  if ($LASTEXITCODE -ne 0) { throw 'cargo update --workspace failed; Cargo.lock is stale' }
  '  ~ Cargo.lock'
}

if (-not $DryRun) {
  $off = @()
  if ((ReadText 'Cargo.toml') -notmatch [regex]::Escape("version = `"$Version`"")) { $off += 'Cargo.toml' }
  foreach ($p in $pkgs) { if ((ReadText $p) -notmatch [regex]::Escape("`"version`": `"$Version`"")) { $off += $p } }
  if ($off) { throw "read-back disagrees: $($off -join ', ')" }
  "all manifests read back $Version (versionCode $code). Next: CHANGELOG entry, commit, then tag v$Version."
}
