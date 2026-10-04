<#
.SYNOPSIS
  Validates docs/tracker.csv — the row-level backlog behind docs/TRACKER.md.

.DESCRIPTION
  Rules (docs/TRACKER.md header):
    - header is exactly id,epic,title,type,priority,status,evidence
    - ids are unique, non-empty, shaped PREFIX-NNN
    - status in todo|doing|done|blocked|dropped
    - done rows carry evidence (command + result, test name, screenshot path)
    - blocked/dropped rows carry the reason in evidence
    - priority in P0..P3
  Exit 1 on any violation, listing every offending line.

.PARAMETER Path
  CSV to validate (default docs/tracker.csv under the repo root).
#>
[CmdletBinding()]
param([string] $Path = (Join-Path (Split-Path -Parent $PSScriptRoot) 'docs/tracker.csv'))

$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $Path)) { Write-Host "FAIL  $Path not found" -ForegroundColor Red; exit 1 }

$expected = 'id,epic,title,type,priority,status,evidence'
$statuses = 'todo', 'doing', 'done', 'blocked', 'dropped'
$errors = [System.Collections.Generic.List[string]]::new()

$raw = [IO.File]::ReadAllLines((Resolve-Path -LiteralPath $Path).Path)
$header = $raw[0].TrimStart([char]0xFEFF).Trim()
if ($header -ne $expected) { $errors.Add("line 1: header is '$header', expected '$expected'") }

$rows = @(Import-Csv -LiteralPath $Path)
$seen = @{}
$line = 1
foreach ($r in $rows) {
  $line++
  $where = "line ${line} ($($r.id))"
  if ([string]::IsNullOrWhiteSpace($r.id)) { $errors.Add("line ${line}: empty id"); continue }
  if ($r.id -notmatch '^[A-Z]+-\d{3}$') { $errors.Add("${where}: id must look like ABC-001") }
  if ($seen.ContainsKey($r.id)) { $errors.Add("${where}: duplicate id (first on line $($seen[$r.id]))") } else { $seen[$r.id] = $line }
  foreach ($col in 'epic', 'title', 'type') {
    if ([string]::IsNullOrWhiteSpace($r.$col)) { $errors.Add("${where}: empty $col") }
  }
  if ($r.priority -notmatch '^P[0-3]$') { $errors.Add("${where}: priority '$($r.priority)' not in P0..P3") }
  if ($r.status -notin $statuses) { $errors.Add("${where}: status '$($r.status)' not in $($statuses -join '|')") }
  elseif ($r.status -eq 'done' -and [string]::IsNullOrWhiteSpace($r.evidence)) { $errors.Add("${where}: done without evidence") }
  elseif ($r.status -in 'blocked', 'dropped' -and [string]::IsNullOrWhiteSpace($r.evidence)) { $errors.Add("${where}: $($r.status) without a reason in evidence") }
}

$counts = $rows | Group-Object status | Sort-Object Name | ForEach-Object { "$($_.Name)=$($_.Count)" }
if ($errors.Count) {
  Write-Host "FAIL  tracker.csv: $($errors.Count) problem(s)" -ForegroundColor Red
  foreach ($e in $errors) { Write-Host "      $e" }
  exit 1
}
Write-Host "PASS  tracker.csv: $($rows.Count) rows, ids unique, statuses valid ($($counts -join ', '))" -ForegroundColor Green
exit 0
