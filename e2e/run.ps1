# The evidence harness from PowerShell: runs e2e/run.sh with Git Bash (or
# WSL's bash), passing every argument through. See e2e/README.md.
#
#   ./e2e/run.ps1 --load-profile load --duration 2m --vus 50 --scenarios checks,baseline
#   ./e2e/run.ps1 up
$ErrorActionPreference = 'Stop'

$script = Join-Path $PSScriptRoot 'run.sh'
$candidates = @(
    (Join-Path $env:ProgramFiles 'Git\bin\bash.exe'),
    (Join-Path ${env:ProgramFiles(x86)} 'Git\bin\bash.exe'),
    (Get-Command bash -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source)
) | Where-Object { $_ -and (Test-Path $_) }

if (-not $candidates) {
    Write-Error 'bash not found: install Git for Windows (Git Bash) or WSL.'
}
$bash = $candidates | Select-Object -First 1
& $bash $script @args
exit $LASTEXITCODE
