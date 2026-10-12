$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$python = Join-Path $projectRoot '.graphify-tools\venv\Scripts\python.exe'
if (-not (Test-Path -LiteralPath $python)) {
    throw 'Graphify is not installed. See docs/graphify.md.'
}
$env:PYTHONUTF8 = '1'
Push-Location $projectRoot
try {
    & $python -m graphify @args
    $graphifyExitCode = $LASTEXITCODE
} finally {
    Pop-Location
}
exit $graphifyExitCode
