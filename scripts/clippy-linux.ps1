# Clippy for Linux through WSL. Host (Windows) clippy never compiles #[cfg(not(windows))]
# code, so code that only exists off Windows (desktop stubs, service stubs) can break CI's
# ubuntu job unseen. Skipped (not failed) without WSL + cargo; CI's ubuntu job always runs.
param([string] $Distro = 'Ubuntu-24.04')
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { cargo clippy --workspace --all-targets --locked -- -D warnings; exit $LASTEXITCODE }
if (-not (Get-Command wsl -ErrorAction SilentlyContinue)) { Write-Host 'SKIP: no WSL'; exit 0 }
$distros = (wsl -l -q) -replace "`0", '' | Where-Object { $_.Trim() }
if ($distros -notcontains $Distro) { Write-Host "SKIP: WSL distro $Distro not installed"; exit 0 }
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$wslRepo = '/mnt/' + $repo.Substring(0, 1).ToLower() + ($repo.Substring(2) -replace '\\', '/')
$probe = wsl -d $Distro -- bash -lc 'command -v cargo >/dev/null && pkg-config --exists webkit2gtk-4.1 && echo ok'
if ("$probe".Trim() -ne 'ok') { Write-Host "SKIP: $Distro lacks cargo or libwebkit2gtk-4.1-dev"; exit 0 }
# Own target dir in the Linux filesystem: fast, and never shares artifacts with Windows builds.
wsl -d $Distro -- bash -lc "cd '$wslRepo' && CARGO_TARGET_DIR=`$HOME/scrin-target cargo clippy --workspace --all-targets --locked -- -D warnings"
exit $LASTEXITCODE
