<#
.SYNOPSIS
  Repo-wide invariants that no compiler or linter checks for scrin.

.DESCRIPTION
  Each invariant prints PASS or FAIL with its id; a FAIL lists every offending
  file:line. Exit 1 when any invariant fails. Every check is mutation-tested by
  scripts/test-invariants.ps1, which points -Root at a fixture tree — keep each
  check rooted at $Root and add a mutation for every new check.

  Rules for writing checks (learned in sibling repos):
    - Check CODE, not prose: a checker that fires on its own documentation
      gets ignored. Scan code files and manifests, skip comment lines.
    - A justified exception is explicit and greppable (a `scrin-allow-*:`
      marker with a reason), never a silent path exclusion.

.PARAMETER Root
  Tree to check (default: the repository root).

.EXAMPLE
  pwsh -NoProfile -File scripts/check-invariants.ps1
  pwsh -NoProfile -File scripts/check-invariants.ps1 -Root .copilot-tmp/inv-test/fixture
#>
[CmdletBinding()]
param([string] $Root = (Split-Path -Parent $PSScriptRoot))

$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path -LiteralPath $Root).Path
Set-Location -LiteralPath $Root

$script:failed = [System.Collections.Generic.List[string]]::new()

function Report([string] $id, [string] $name, [object[]] $hits, [string] $hint = '', [string] $note = '') {
  $hits = @($hits | Where-Object { $_ })
  if ($hits.Count -eq 0) {
    $suffix = if ($note) { "  ($note)" } else { '' }
    Write-Host "PASS  $id $name$suffix" -ForegroundColor DarkGreen
    return
  }
  $script:failed.Add($id)
  Write-Host "FAIL  $id $name" -ForegroundColor Red
  if ($hint) { Write-Host "      fix: $hint" -ForegroundColor Yellow }
  foreach ($h in $hits | Select-Object -First 50) { Write-Host "      $h" }
  if ($hits.Count -gt 50) { Write-Host "      ... and $($hits.Count - 50) more" }
}

# rg wrapper: returns 'file:line: text' for each match; paths that do not exist are skipped.
function Search([string[]] $Paths, [string[]] $Patterns, [string[]] $Globs = @(), [switch] $NoComments) {
  $existing = @($Paths | Where-Object { Test-Path -LiteralPath $_ })
  if (-not $existing) { return @() }
  # Parent/global ignore files are skipped so a fixture under .copilot-tmp/ (itself
  # gitignored) is scanned like a real tree; the repo's own .gitignore still applies.
  $rgArgs = @('-n', '--no-heading', '--with-filename', '--color', 'never', '--no-ignore-parent', '--no-ignore-global', '--hidden', '-g', '!.git/')
  foreach ($g in $Globs) { $rgArgs += @('-g', $g) }
  foreach ($p in $Patterns) { $rgArgs += @('-e', $p) }
  $out = @(& rg @rgArgs -- @existing 2>$null)
  foreach ($l in $out) {
    if ($l -notmatch '^(?<f>.*?):(?<n>\d+):(?<t>.*)$') { continue }
    $t = $Matches['t']
    if ($NoComments -and $t -match '^\s*(//|#|\*|/\*)') { continue }
    '{0}:{1}: {2}' -f (($Matches['f'] -replace '\\', '/') -replace '^\./', ''), $Matches['n'], $t.Trim()
  }
}

function Files([string[]] $Paths, [string[]] $Globs) {
  $existing = @($Paths | Where-Object { Test-Path -LiteralPath $_ })
  if (-not $existing) { return @() }
  $rgArgs = @('--files', '--no-ignore-parent', '--no-ignore-global')
  foreach ($g in $Globs) { $rgArgs += @('-g', $g) }
  @(& rg @rgArgs -- @existing 2>$null | ForEach-Object { ($_ -replace '\\', '/') -replace '^\./', '' })
}

$codeGlobs = @('*.{ts,tsx,js,jsx,mjs,cjs,mts,cts}', '!*.d.ts')
$jsRoots = @('apps', 'packages', 'e2e')
$rustSrc = @(Files @('crates') @('*.rs') | Where-Object { $_ -match '^crates/[^/]+/src/' })
$manifests = @(Files @('.') @('package.json'))

# ---- INV-01 no unwrap() in non-test Rust ----------------------------------
# Library code returns Err/None; a panic in the capture or network path drops a
# live session. Test modules (everything from the first #[cfg(test)]) and
# `// scrin-allow-unwrap: <why>` on the same or previous line are exempt.
$hits = foreach ($f in $rustSrc) {
  $lines = [IO.File]::ReadAllLines((Join-Path $Root $f))
  for ($i = 0; $i -lt $lines.Count; $i++) {
    $l = $lines[$i]
    if ($l -match '^\s*#\[cfg\(test\)\]') { break }
    if ($l -match '^\s*//') { continue }
    if ($l -notmatch '\.unwrap\(\)') { continue }
    if ($l -match 'scrin-allow-unwrap:' -or ($i -gt 0 -and $lines[$i - 1] -match 'scrin-allow-unwrap:')) { continue }
    "${f}:$($i + 1): $($l.Trim())"
  }
}
Report 'INV-01' 'no unwrap() in non-test Rust' $hits 'return a Result/Option, or mark `// scrin-allow-unwrap: <why it cannot fail>`'

# ---- INV-02 every unsafe block has a SAFETY comment -----------------------
$hits = foreach ($f in $rustSrc) {
  $lines = [IO.File]::ReadAllLines((Join-Path $Root $f))
  for ($i = 0; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match '^\s*//' -or $lines[$i] -notmatch '\bunsafe\s*\{') { continue }
    $from = [Math]::Max(0, $i - 3)
    $ctx = ($lines[$from..$i]) -join "`n"
    if ($ctx -notmatch 'SAFETY:') { "${f}:$($i + 1): $($lines[$i].Trim())" }
  }
}
Report 'INV-02' 'every unsafe block has a // SAFETY: comment' $hits 'state the invariant that makes it sound, within 3 lines above'

# ---- INV-03 every crate inherits workspace lints --------------------------
$hits = foreach ($m in (Files @('crates') @('Cargo.toml'))) {
  $text = [IO.File]::ReadAllText((Join-Path $Root $m))
  if ($text -notmatch '(?m)^\[lints\]\s*\r?\n\s*workspace\s*=\s*true') { "${m}: missing [lints] workspace = true" }
}
Report 'INV-03' 'every crate has [lints] workspace = true' $hits 'clippy pedantic + unwrap_used are workspace lints; a crate without them is unlinted'

# ---- INV-04 licence is AGPL-3.0-only in every manifest --------------------
$hits = @()
foreach ($m in $manifests) {
  try { $j = [IO.File]::ReadAllText((Join-Path $Root $m)) | ConvertFrom-Json } catch { $hits += "${m}: unparseable JSON"; continue }
  if ($j.license -ne 'AGPL-3.0-only') { $hits += "${m}: license = '$($j.license)'" }
}
foreach ($m in (Files @('.') @('Cargo.toml'))) {
  $text = [IO.File]::ReadAllText((Join-Path $Root $m))
  $wp = [regex]::Match($text, '(?ms)^\[workspace\.package\][^\r\n]*\r?\n(?<body>.*?)(?=^\[|\z)')
  if ($wp.Success -and $wp.Groups['body'].Value -notmatch '(?m)^license\s*=\s*"AGPL-3\.0-only"') {
    $hits += "${m}: [workspace.package] license is not AGPL-3.0-only"
  }
  if ($text -match '(?m)^\[package\]' -and $text -notmatch '(?m)^license(\.workspace\s*=\s*true|\s*=\s*"AGPL-3\.0-only")') {
    $hits += "${m}: [package] needs license.workspace = true"
  }
}
Report 'INV-04' 'licence AGPL-3.0-only in every package.json and Cargo.toml' $hits 'set "license": "AGPL-3.0-only" / license.workspace = true (ADR-0001)'

# ---- INV-05 Motion, never framer-motion -----------------------------------
$hits = @(Search $jsRoots @('(from|import|require)\s*\(?\s*[''"]framer-motion') $codeGlobs -NoComments) +
@(Search $manifests @('"framer-motion"\s*:'))
Report 'INV-05' 'no framer-motion (use motion/react)' $hits 'import from "motion/react" (ADR-0010)'

# ---- INV-06 Base UI only, no Radix ----------------------------------------
$hits = @(Search $jsRoots @('[''"](@radix-ui/[^''"]+|radix-ui)[''"]') $codeGlobs -NoComments) +
@(Search $manifests @('"(@radix-ui/[^"]+|radix-ui)"\s*:'))
Report 'INV-06' 'no Radix (Base UI only)' $hits 'use @base-ui/react via packages/ui (ADR-0010)'

# ---- INV-07 no console.log in shipped TS ----------------------------------
$srcRoots = @(foreach ($r in 'apps', 'packages') { if (Test-Path $r) { Get-ChildItem -LiteralPath $r -Directory | ForEach-Object { "$r/$($_.Name)/src" } } })
$hits = @(Search $srcRoots @('\bconsole\.log\(') ($codeGlobs + @('!*.test.*', '!*.spec.*', '!**/__tests__/**')) -NoComments |
  Where-Object { $_ -notmatch 'scrin-allow-console:' })
Report 'INV-07' 'no console.log in apps/*/src and packages/*/src' $hits 'use the logger, or mark `// scrin-allow-console: <why>`'

# ---- INV-08 every locale exists in EN and RO, with a drift test -----------
$hits = @()
$i18nNote = ''
if (Test-Path 'packages/i18n') {
  $loc = @(Files @('packages/i18n') @('**/locales/*.{ts,json}', '!*.test.*', '!*.d.ts'))
  $en = @($loc | Where-Object { $_ -match '/en\.(ts|json)$' })
  $ro = @($loc | Where-Object { $_ -match '/ro\.(ts|json)$' })
  if (-not $en) { $hits += 'packages/i18n: no locales/en.(ts|json)' }
  if (-not $ro) { $hits += 'packages/i18n: no locales/ro.(ts|json)' }
  if (-not (Files @('packages/i18n') @('*drift*.test.*'))) { $hits += 'packages/i18n: no *drift*.test.* (every EN key must exist in RO)' }
} else { $i18nNote = 'packages/i18n not present yet' }
foreach ($v in (Files @('android') @('**/res/values/strings.xml'))) {
  $roFile = $v -replace '/values/strings\.xml$', '/values-ro/strings.xml'
  if (-not (Test-Path -LiteralPath $roFile)) { $hits += "${v}: no matching $roFile" }
}
Report 'INV-08' 'EN + RO locales exist with a drift test' $hits 'add the RO locale and a drift test (ADR-0010)' $i18nNote

# ---- INV-09 email never via Resend ----------------------------------------
$hits = @(Search $jsRoots @('(from|require\()\s*[''"]resend[''"]') $codeGlobs -NoComments) +
@(Search $manifests @('"resend"\s*:'))
Report 'INV-09' 'no Resend (email goes through brivio)' $hits 'send mail through the brivio API (ADR-0007)'

# ---- INV-10 no tsup (unmaintained; use tsdown) ----------------------------
$hits = @(Search $manifests @('"tsup"\s*:')) + @(Files @('.') @('tsup.config.*') | ForEach-Object { "${_}: tsup config" })
Report 'INV-10' 'no tsup (use tsdown)' $hits 'npx tsdown-migrate'

# ---- INV-11 no committed secrets ------------------------------------------
$secretPatterns = @(
  '-----BEGIN (RSA |EC |DSA |OPENSSH |PGP |ENCRYPTED )?PRIVATE KEY-----'
  'AKIA[0-9A-Z]{16}'
  'gh[pousr]_[A-Za-z0-9]{36}'
  'github_pat_[A-Za-z0-9_]{60,}'
  'AIza[0-9A-Za-z_-]{35}'
  '(sk|rk)_live_[0-9A-Za-z]{20,}'
  'xox[abprs]-[0-9A-Za-z-]{10,}'
  'npm_[A-Za-z0-9]{36}'
)
$hits = @(Search @('.') $secretPatterns @('!LICENSE', '!*.lock', '!pnpm-lock.yaml') | ForEach-Object { ($_ -replace '(:\d+:).*$', '$1 <redacted>') })
Report 'INV-11' 'no secret material in tracked files' $hits 'remove it AND rotate the credential; history keeps it'

# ---- INV-12 tracker ids unique, decisions numbered sequentially -----------
$hits = @()
if (Test-Path 'docs/tracker.csv') {
  $rows = @(Import-Csv 'docs/tracker.csv')
  $hits += @($rows | Group-Object id | Where-Object Count -GT 1 | ForEach-Object { "docs/tracker.csv: duplicate id $($_.Name) x$($_.Count)" })
  $hits += @($rows | Where-Object { -not $_.id } | ForEach-Object { 'docs/tracker.csv: row with empty id' })
} else { $hits += 'docs/tracker.csv missing' }
if (Test-Path 'docs/TRACKER.md') {
  $md = [IO.File]::ReadAllLines((Join-Path $Root 'docs/TRACKER.md'))
  $expect = 1
  for ($i = 0; $i -lt $md.Count; $i++) {
    if ($md[$i] -match '^\|\s*D(\d+)\s*\|') {
      $n = [int]$Matches[1]
      if ($n -ne $expect) { $hits += "docs/TRACKER.md:$($i + 1): D$($Matches[1]) out of sequence (expected D$('{0:D2}' -f $expect))" }
      $expect = $n + 1
    }
  }
  if ($expect -eq 1) { $hits += 'docs/TRACKER.md: no | Dnn | decision rows found' }
} else { $hits += 'docs/TRACKER.md missing' }
Report 'INV-12' 'tracker ids unique; TRACKER.md decisions D01..Dnn sequential' $hits 'renumber; never reuse an id'

# ---- INV-13 no focused tests ----------------------------------------------
$hits = @(Search $jsRoots @('\b(it|test|describe|suite|bench)(\.describe)?\.only\(') @('*.{test,spec}.{ts,tsx,js,mjs}', '**/e2e/**/*.{ts,tsx,js,mjs}') -NoComments)
Report 'INV-13' 'no .only( in tests' $hits 'remove .only — it silently skips the rest of the suite'

# ---- INV-14 Android a11y service is not declared an accessibility tool ----
$hits = @(Search @('android') @('isAccessibilityTool\s*=\s*"true"') @('*.xml'))
Report 'INV-14' 'no isAccessibilityTool="true" in android' $hits 'scrin is not an assistive tool; the flag is a false declaration (ADR-0006)'

Write-Host ''
if ($script:failed.Count -gt 0) {
  Write-Host "$($script:failed.Count) invariant(s) violated: $($script:failed -join ', ')" -ForegroundColor Red
  exit 1
}
Write-Host 'All invariants hold.' -ForegroundColor Green
exit 0
