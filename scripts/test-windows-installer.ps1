[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Installer,
    [Parameter(Mandatory)][string]$PackageDirectory
)
$ErrorActionPreference = 'Stop'
# This smoke test changes the current user's uninstall registry entry: run on CI.
if ($env:CI -ne 'true' -or -not $env:RUNNER_TEMP) { throw 'Run the installer smoke test on an isolated GitHub Actions runner' }
$installerPath = (Resolve-Path -LiteralPath $Installer).Path
$packagePath = (Resolve-Path -LiteralPath $PackageDirectory).Path
$testRoot = Join-Path $env:RUNNER_TEMP ('SWCH installer smoke ' + [guid]::NewGuid().ToString('N'))
$installPath = Join-Path $testRoot 'Installed SWCH'
New-Item -ItemType Directory -Force -Path $testRoot | Out-Null
function Install-TestPackage([string]$logName) {
    $logPath = Join-Path $testRoot $logName
    $arguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/NOICONS', '/TASKS=', '/NOCLOSEAPPLICATIONS', '/NORESTARTAPPLICATIONS', ('"/DIR=' + $installPath + '"'), ('"/LOG=' + $logPath + '"'))
    $process = Start-Process -FilePath $installerPath -ArgumentList $arguments -WindowStyle Hidden -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Installer failed: $($process.ExitCode). Log: $logPath" }
}
Install-TestPackage 'install.log'
foreach ($source in Get-ChildItem -LiteralPath $packagePath -Recurse -File) {
    $relative = [IO.Path]::GetRelativePath($packagePath, $source.FullName)
    $destination = Join-Path $installPath $relative
    if (-not (Test-Path -LiteralPath $destination -PathType Leaf) -or
        (Get-FileHash -LiteralPath $source.FullName).Hash -ne (Get-FileHash -LiteralPath $destination).Hash) {
        throw "Installed file differs: $relative"
    }
}
$sentinel = Join-Path $installPath 'user-data-preservation-test.txt'
Set-Content -LiteralPath $sentinel -Value 'preserve user data' -Encoding utf8
Install-TestPackage 'reinstall.log'
if ((Get-Content -LiteralPath $sentinel -Raw).Trim() -ne 'preserve user data') { throw 'Reinstallation changed user data' }
$uninstaller = Join-Path $installPath 'unins000.exe'
if (-not (Test-Path -LiteralPath $uninstaller)) { throw 'Uninstaller is missing' }
$process = Start-Process -FilePath $uninstaller -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART') -WindowStyle Hidden -Wait -PassThru
if ($process.ExitCode -ne 0) { throw 'Uninstallation failed' }
if (Test-Path -LiteralPath (Join-Path $installPath 'swch.exe')) { throw 'Uninstallation left the application executable' }
if ((Get-Content -LiteralPath $sentinel -Raw).Trim() -ne 'preserve user data') { throw 'Uninstallation removed user data' }
Write-Host 'Installer smoke test passed: payload hashes, paths with spaces, reinstall and uninstall preserve user data.'
