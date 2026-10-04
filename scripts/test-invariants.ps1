<#
.SYNOPSIS
  Mutation-tests check-invariants.ps1: proves every invariant can actually fail.

.DESCRIPTION
  A green check is not evidence. This builds a minimal CLEAN fixture tree under
  .copilot-tmp/inv-test/<pid>/, asserts the checker passes on it (baseline),
  then for each invariant injects one violation, runs the checker with -Root
  pointed at the fixture, asserts it FAILS naming that invariant id, and
  undoes the mutation. The real tree is never modified, so concurrent runs and
  other agents are safe.

  Also asserts the escape hatches work (a marked unwrap passes, test-module
  unwrap passes, a comment mentioning framer-motion passes) — a checker that
  fires on prose or on tests gets ignored.

  Add a case here whenever you add an invariant; the final coverage check fails
  when an INV-id in check-invariants.ps1 has no mutation.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$checker = Join-Path $PSScriptRoot 'check-invariants.ps1'
$fx = Join-Path $repo ".copilot-tmp/inv-test/$PID"
$script:fails = 0
$script:tested = [System.Collections.Generic.HashSet[string]]::new()

function W([string] $rel, [string] $text) {
  $p = Join-Path $fx $rel
  New-Item -ItemType Directory -Force (Split-Path $p) | Out-Null
  [IO.File]::WriteAllText($p, $text.Replace("`r`n", "`n"), [Text.UTF8Encoding]::new($false))
}

function New-Fixture {
  if (Test-Path $fx) { Remove-Item -Recurse -Force $fx }
  W 'Cargo.toml' "[workspace]`nresolver = `"3`"`nmembers = [`"crates/demo`"]`n`n[workspace.package]`nversion = `"0.1.0`"`nlicense = `"AGPL-3.0-only`"`n`n[workspace.lints.clippy]`nall = `"warn`"`n"
  W 'crates/demo/Cargo.toml' "[package]`nname = `"demo`"`nversion.workspace = true`nlicense.workspace = true`n`n[lints]`nworkspace = true`n"
  W 'crates/demo/src/lib.rs' "pub fn add(a: u32, b: u32) -> u32 {`n    a + b`n}`n"
  W 'package.json' '{ "name": "fixture", "private": true, "license": "AGPL-3.0-only" }'
  W 'packages/ui/package.json' '{ "name": "@scrin/ui", "license": "AGPL-3.0-only", "dependencies": { "@base-ui/react": "1.8.0", "motion": "14.0.0" } }'
  W 'packages/ui/src/button.tsx' "// framer-motion is banned here; this comment must not trip INV-05`nimport { motion } from `"motion/react`";`nexport const Button = motion.button;`n"
  W 'packages/i18n/package.json' '{ "name": "@scrin/i18n", "license": "AGPL-3.0-only" }'
  W 'packages/i18n/src/locales/en.ts' "export const en = { hello: `"Hello`" };`n"
  W 'packages/i18n/src/locales/ro.ts' "export const ro = { hello: `"Salut`" };`n"
  W 'packages/i18n/src/locales/drift.test.ts' "import { it } from `"vitest`";`nit(`"ro has every en key`", () => {});`n"
  W 'docs/tracker.csv' "id,epic,title,type,priority,status,evidence`nF-001,Foundation,Repo,task,P0,todo,`n"
  W 'docs/TRACKER.md' "| # | Decision | Reason |`n|---|---|---|`n| D01 | a | b |`n| D02 | c | d |`n"
  W 'android/app/src/main/res/values/strings.xml' "<resources><string name=`"app`">scrin</string></resources>`n"
  W 'android/app/src/main/res/values-ro/strings.xml' "<resources><string name=`"app`">scrin</string></resources>`n"
  W 'android/app/src/main/res/xml/a11y.xml' "<accessibility-service android:isAccessibilityTool=`"false`" />`n"
}

function Invoke-Checker {
  $out = & pwsh -NoProfile -File $checker -Root $fx 2>&1 | Out-String
  [pscustomobject]@{ Code = $LASTEXITCODE; Out = $out }
}

# Apply $Mutate (scriptblock) to a fresh fixture, expect FAIL for $Id.
function Test-Catches([string] $Id, [string] $Name, [scriptblock] $Mutate) {
  $null = $script:tested.Add($Id)
  New-Fixture
  & $Mutate
  $r = Invoke-Checker
  if ($r.Code -ne 0 -and $r.Out -match "FAIL\s+$Id\b") { Write-Host "  + $Id catches $Name" -ForegroundColor DarkGreen }
  else {
    Write-Host "  x $Id did NOT catch $Name (exit $($r.Code))" -ForegroundColor Red
    Write-Host ($r.Out -split "`n" | Where-Object { $_ -match $Id } | Out-String)
    $script:fails++
  }
}

# Apply $Mutate, expect the checker to stay green (false-positive guard).
function Test-Allows([string] $Name, [scriptblock] $Mutate) {
  New-Fixture
  & $Mutate
  $r = Invoke-Checker
  if ($r.Code -eq 0) { Write-Host "  + allows $Name" -ForegroundColor DarkGreen }
  else {
    Write-Host "  x false positive on $Name" -ForegroundColor Red
    Write-Host ($r.Out -split "`n" | Where-Object { $_ -match 'FAIL|      ' } | Out-String)
    $script:fails++
  }
}

try {
  Write-Host 'Mutation-testing scripts/check-invariants.ps1 ...' -ForegroundColor Cyan

  New-Fixture
  $base = Invoke-Checker
  if ($base.Code -ne 0) {
    Write-Host '  x baseline fixture already fails — every result below would be meaningless' -ForegroundColor Red
    Write-Host $base.Out
    exit 1
  }
  Write-Host '  + baseline: clean fixture passes' -ForegroundColor DarkGreen

  Test-Catches 'INV-01' 'unwrap() in library code' { W 'crates/demo/src/io.rs' "pub fn f(s: &str) -> u32 {`n    s.parse().unwrap()`n}`n" }
  Test-Catches 'INV-02' 'unsafe without SAFETY' { W 'crates/demo/src/ffi.rs' "pub fn f(p: *const u8) -> u8 {`n    unsafe { *p }`n}`n" }
  Test-Catches 'INV-03' 'crate without workspace lints' { W 'crates/demo/Cargo.toml' "[package]`nname = `"demo`"`nlicense.workspace = true`n" }
  Test-Catches 'INV-04' 'MIT package.json' { W 'packages/ui/package.json' '{ "name": "@scrin/ui", "license": "MIT" }' }
  Test-Catches 'INV-04' 'crate overriding the licence' { W 'crates/demo/Cargo.toml' "[package]`nname = `"demo`"`nlicense = `"MIT`"`n`n[lints]`nworkspace = true`n" }
  Test-Catches 'INV-04' 'workspace licence changed' { W 'Cargo.toml' "[workspace]`nmembers = []`n`n[workspace.package]`nlicense = `"MIT`"`n" }
  Test-Catches 'INV-05' 'framer-motion import' { W 'packages/ui/src/fade.tsx' "import { motion } from 'framer-motion';`nexport const F = motion.div;`n" }
  Test-Catches 'INV-05' 'framer-motion dependency' { W 'packages/ui/package.json' '{ "name": "@scrin/ui", "license": "AGPL-3.0-only", "dependencies": { "framer-motion": "12.0.0" } }' }
  Test-Catches 'INV-06' '@radix-ui import' { W 'packages/ui/src/dialog.tsx' "import * as D from `"@radix-ui/react-dialog`";`nexport const X = D.Root;`n" }
  Test-Catches 'INV-06' 'radix-ui dependency' { W 'packages/ui/package.json' '{ "name": "@scrin/ui", "license": "AGPL-3.0-only", "dependencies": { "radix-ui": "1.4.0" } }' }
  Test-Catches 'INV-07' 'console.log in src' { W 'packages/ui/src/debug.ts' "export function d(x: unknown) {`n  console.log(x);`n}`n" }
  Test-Catches 'INV-08' 'missing RO locale' { Remove-Item (Join-Path $fx 'packages/i18n/src/locales/ro.ts') }
  Test-Catches 'INV-08' 'missing drift test' { Remove-Item (Join-Path $fx 'packages/i18n/src/locales/drift.test.ts') }
  Test-Catches 'INV-08' 'android strings without values-ro' { Remove-Item -Recurse (Join-Path $fx 'android/app/src/main/res/values-ro') }
  Test-Catches 'INV-09' 'resend dependency' { W 'apps/api/package.json' '{ "name": "@scrin/api", "license": "AGPL-3.0-only", "dependencies": { "resend": "6.0.0" } }' }
  Test-Catches 'INV-09' 'resend import' { W 'apps/api/src/mail.ts' "import { Resend } from `"resend`";`nexport const r = Resend;`n" }
  Test-Catches 'INV-10' 'tsup devDependency' { W 'packages/ui/package.json' '{ "name": "@scrin/ui", "license": "AGPL-3.0-only", "devDependencies": { "tsup": "8.0.0" } }' }
  # Secret-shaped strings are assembled at runtime so this file never contains one.
  Test-Catches 'INV-11' 'AWS access key' { W 'apps/api/src/cfg.ts' ("export const k = `"" + 'AKI' + 'A' + ('Q' * 16) + "`";`n") }
  Test-Catches 'INV-11' 'private key block' { W 'deploy/key.txt' ('-----BEGIN ' + 'PRIVATE KEY-----' + "`nMIIE`n") }
  Test-Catches 'INV-11' 'GitHub token' { W 'scripts/x.ps1' ("`$t = '" + 'gh' + 'p_' + ('a1' * 18) + "'`n") }
  Test-Catches 'INV-12' 'duplicate tracker id' { W 'docs/tracker.csv' "id,epic,title,type,priority,status,evidence`nF-001,Foundation,Repo,task,P0,todo,`nF-001,Foundation,Again,task,P0,todo,`n" }
  Test-Catches 'INV-12' 'decision gap D01 -> D03' { W 'docs/TRACKER.md' "| # | Decision | Reason |`n|---|---|---|`n| D01 | a | b |`n| D03 | c | d |`n" }
  Test-Catches 'INV-13' 'it.only in a test' { W 'packages/ui/src/button.test.tsx' "import { it } from `"vitest`";`nit.only(`"renders`", () => {});`n" }
  Test-Catches 'INV-13' 'test.describe.only in e2e' { W 'apps/web/e2e/home.spec.ts' "import { test } from `"@playwright/test`";`ntest.describe.only(`"home`", () => {});`n" }
  Test-Catches 'INV-14' 'isAccessibilityTool true' { W 'android/app/src/main/res/xml/a11y.xml' "<accessibility-service android:isAccessibilityTool=`"true`" />`n" }

  # False-positive guards: the escape hatches and test code must stay green.
  Test-Allows 'unwrap with a scrin-allow-unwrap marker' { W 'crates/demo/src/io.rs' "pub fn f() -> u32 {`n    // scrin-allow-unwrap: literal is a valid u32`n    `"7`".parse().unwrap()`n}`n" }
  Test-Allows 'unwrap inside #[cfg(test)]' { W 'crates/demo/src/t.rs' "pub fn one() -> u32 { 1 }`n`n#[cfg(test)]`nmod tests {`n    #[test]`n    fn parses() {`n        assert_eq!(`"1`".parse::<u32>().unwrap(), super::one());`n    }`n}`n" }
  Test-Allows 'unsafe with SAFETY comment' { W 'crates/demo/src/ffi.rs' "pub fn f(p: &u8) -> u8 {`n    // SAFETY: p is a valid reference`n    unsafe { *(p as *const u8) }`n}`n" }
  Test-Allows 'console.log in a test file' { W 'packages/ui/src/x.test.ts' "console.log(`"debug in test`");`n" }

  # Coverage: every INV id defined in the checker has at least one mutation.
  $defined = @([regex]::Matches([IO.File]::ReadAllText($checker), "Report '(INV-\d+)'") | ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique)
  $untested = @($defined | Where-Object { -not $script:tested.Contains($_) })
  if ($untested) { Write-Host "  x invariants without a mutation test: $($untested -join ', ')" -ForegroundColor Red; $script:fails++ }
  else { Write-Host "  + coverage: all $($defined.Count) invariants mutation-tested" -ForegroundColor DarkGreen }
}
finally {
  if (Test-Path $fx) { Remove-Item -Recurse -Force $fx }
  $parent = Split-Path $fx
  if ((Test-Path $parent) -and -not (Get-ChildItem $parent)) { Remove-Item $parent -Force }
}

Write-Host ''
if ($script:fails -gt 0) {
  Write-Host "$($script:fails) mutation(s) not caught or false positive(s) — those checks are decorative." -ForegroundColor Red
  exit 1
}
Write-Host 'Every invariant check provably catches its violation.' -ForegroundColor Green
exit 0
