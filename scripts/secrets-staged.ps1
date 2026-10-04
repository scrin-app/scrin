# Pre-commit secret scan. Skipped (not failed) when gitleaks is absent; CI always runs it.
$ErrorActionPreference = 'Stop'
if (-not (Get-Command gitleaks -ErrorAction SilentlyContinue)) {
    Write-Host 'gitleaks not installed - SKIPPED (CI runs it)'
    exit 0
}
git rev-parse --verify -q HEAD *> $null
if ($LASTEXITCODE -eq 0) {
    gitleaks git --staged --no-banner --redact --exit-code 1 --config .gitleaks.toml
} else {
    # First commit: `git --staged` needs a HEAD, so scan the working tree instead.
    gitleaks dir . --no-banner --redact --exit-code 1 --config .gitleaks.toml
}
exit $LASTEXITCODE
