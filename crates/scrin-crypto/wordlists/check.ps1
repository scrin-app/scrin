#Requires -Version 7
# Validates the passphrase wordlists (D24). Exit 0 = all rules pass, 1 = violation.
# Usage: pwsh -NoProfile -File crates/scrin-crypto/wordlists/check.ps1
$ErrorActionPreference = 'Stop'
$dir = $PSScriptRoot
$expected = 1024
$errors = [System.Collections.Generic.List[string]]::new()

function Get-Folded([string]$w) {
    return $w.Replace([string][char]0x0103, 'a').Replace([string][char]0x00E2, 'a').
        Replace([string][char]0x00EE, 'i').Replace([string][char]0x0219, 's').Replace([string][char]0x021B, 't')
}

function Get-Prefix([string]$f) {
    if ($f.Length -ge 4) { return $f.Substring(0, 4) }
    return $f
}

function Read-List([string]$name, [string]$charClass, [int]$min, [int]$max) {
    $path = Join-Path $dir $name
    if (-not (Test-Path -LiteralPath $path)) { $errors.Add("${name}: file missing"); return @() }
    $bytes = [System.IO.File]::ReadAllBytes($path)
    if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
        $errors.Add("${name}: has a UTF-8 BOM")
    }
    if ($bytes -contains 13) { $errors.Add("${name}: contains CR (must be LF line endings)") }
    if ($bytes.Length -eq 0 -or $bytes[-1] -ne 10) { $errors.Add("${name}: missing trailing newline") }
    $text = [System.Text.UTF8Encoding]::new($false, $true).GetString($bytes)
    if ($text.Contains([char]0x015F) -or $text.Contains([char]0x0163) -or
        $text.Contains([char]0x015E) -or $text.Contains([char]0x0162)) {
        $errors.Add("${name}: contains cedilla s/t (use comma-below U+0219/U+021B)")
    }
    if ($text.EndsWith("`n")) { $text = $text.Substring(0, $text.Length - 1) }
    $words = $text.Split("`n")
    if ($words.Count -ne $expected) { $errors.Add("${name}: $($words.Count) words, expected $expected") }
    $re = "^[$charClass]{$min,$max}$"
    $seenWord = @{}
    $i = 0
    foreach ($w in $words) {
        $i++
        if (-not $w.IsNormalized([System.Text.NormalizationForm]::FormC)) { $errors.Add("${name}:${i}: '$w' is not NFC") }
        if ($w -cnotmatch $re) { $errors.Add("${name}:${i}: '$w' violates charset/length [$charClass]{$min,$max}") }
        if ($seenWord.ContainsKey($w)) { $errors.Add("${name}:${i}: duplicate '$w' (line $($seenWord[$w]))") } else { $seenWord[$w] = $i }
    }
    return $words
}

$en = Read-List 'en.txt' 'a-z' 3 8
$ro = Read-List 'ro.txt' ('a-z' + [char]0x0103 + [char]0x00E2 + [char]0x00EE + [char]0x0219 + [char]0x021B) 3 9

# Folded uniqueness + prefix rules, within and across both lists.
$folded = @{}   # folded word -> "list:line"
$prefixes = @{} # folded prefix (<= 4 chars) -> "list:line word"
foreach ($entry in @(@{ Name = 'en.txt'; Words = $en }, @{ Name = 'ro.txt'; Words = $ro })) {
    $i = 0
    foreach ($w in $entry.Words) {
        $i++
        $where = "$($entry.Name):$i"
        $f = Get-Folded $w
        if ($folded.ContainsKey($f)) { $errors.Add("${where}: '$w' folds to '$f', same as $($folded[$f])") } else { $folded[$f] = "$where '$w'" }
        $p = Get-Prefix $f
        if ($prefixes.ContainsKey($p)) {
            $errors.Add("${where}: '$w' folded prefix '$p' collides with $($prefixes[$p])")
        } else {
            $prefixes[$p] = "$where '$w'"
        }
    }
}
# A short word (< 4 letters) must not be the start of another word's 4-letter prefix.
foreach ($short in @($prefixes.Keys | Where-Object { $_.Length -lt 4 })) {
    foreach ($p in $prefixes.Keys) {
        if ($p -ne $short -and $p.StartsWith($short, [System.StringComparison]::Ordinal)) {
            $errors.Add("short word $($prefixes[$short]) is a prefix of $($prefixes[$p])")
        }
    }
}

if ($errors.Count -gt 0) {
    $errors | Select-Object -First 50 | ForEach-Object { Write-Host "FAIL $_" }
    if ($errors.Count -gt 50) { Write-Host "... and $($errors.Count - 50) more" }
    Write-Host "wordlists: $($errors.Count) violation(s)"
    exit 1
}
Write-Host "wordlists OK: en.txt=$($en.Count) ro.txt=$($ro.Count), folded words and 4-letter prefixes unique within and across lists"
exit 0
