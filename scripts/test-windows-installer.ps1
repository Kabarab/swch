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
    $arguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/TASKS=', '/NOCLOSEAPPLICATIONS', '/NORESTARTAPPLICATIONS', ('"/DIR=' + $installPath + '"'), ('"/LOG=' + $logPath + '"'))
    $process = Start-Process -FilePath $installerPath -ArgumentList $arguments -WindowStyle Hidden -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Installer failed: $($process.ExitCode). Log: $logPath" }
}
Install-TestPackage 'install.log'
$shortcutPath = Join-Path ([Environment]::GetFolderPath('Programs')) 'SWCH.lnk'
if (-not (Test-Path -LiteralPath $shortcutPath)) { throw 'Start menu application shortcut is missing' }
$shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcutPath)
if ($shortcut.TargetPath -ne (Join-Path $installPath 'swch.exe') -or $shortcut.WorkingDirectory -ne $installPath) {
    throw 'Start menu shortcut points to the wrong application'
}
$shell = New-Object -ComObject Shell.Application
$item = $shell.NameSpace((Split-Path -Parent $shortcutPath)).ParseName('SWCH.lnk')
if ($item.ExtendedProperty('System.AppUserModel.ID') -ne 'com.kabarab.swch') {
    throw 'Start menu shortcut has no SWCH application identity'
}
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{71E78E5E-0A60-4F4F-A637-8040CCDB9154}_is1'
$registration = Get-ItemProperty -LiteralPath $uninstallKey
if ($registration.DisplayName -notmatch '^SWCH' -or $registration.InstallLocation.TrimEnd('\') -ne $installPath -or
    -not $registration.DisplayIcon.Contains((Join-Path $installPath 'swch.exe'))) {
    throw 'Installed Apps entry is missing or has the wrong name, path or icon'
}
$versionInfo = (Get-Item -LiteralPath (Join-Path $installPath 'swch.exe')).VersionInfo
if ($versionInfo.ProductName -ne 'SWCH' -or $versionInfo.FileDescription -ne 'SWCH' -or
    $versionInfo.ProductVersion -ne $registration.DisplayVersion) {
    throw 'Windows does not identify the executable as the released SWCH application'
}
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
if (Test-Path -LiteralPath $shortcutPath) { throw 'Uninstallation left the Start menu shortcut' }
if (Test-Path -LiteralPath $uninstallKey) { throw 'Uninstallation left the Installed Apps entry' }
if ((Get-Content -LiteralPath $sentinel -Raw).Trim() -ne 'preserve user data') { throw 'Uninstallation removed user data' }
Write-Host 'Installer smoke test passed: Start menu identity, Installed Apps registration, product metadata, payload hashes and data preservation.'
