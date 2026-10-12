# Synchronize only the UI from the canonical workspace, keeping a recoverable copy.
[CmdletBinding()]
param([string]$InstalledDirectory = 'C:\Users\NAZAR\Desktop', [switch]$RestartServer)
$ErrorActionPreference='Stop'
$taskWorkspace=(Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$taskInstalled=(Resolve-Path -LiteralPath $InstalledDirectory).Path
if ($taskWorkspace -eq $taskInstalled) {throw 'Installed directory must differ from the workspace'}
$taskIndex=Join-Path $taskWorkspace 'index.html'
$taskUi=Join-Path $taskWorkspace 'ui'
if (-not (Test-Path -LiteralPath $taskIndex) -or -not (Test-Path -LiteralPath $taskUi)) {throw 'Canonical UI is missing'}
$taskBackend=Join-Path $taskInstalled 'swch-server.exe'
$taskRunning=@()
if ($RestartServer) {$taskRunning=@(Get-CimInstance Win32_Process -Filter "Name='swch-server.exe'" | Where-Object {$_.ExecutablePath -eq $taskBackend})}
if ($RestartServer -and $taskRunning.Count) {
    $taskClaims=Invoke-RestMethod -Uri 'http://127.0.0.1:4242/api/free-games' -TimeoutSec 5
    if ($taskClaims.run.running) {throw 'A giveaway run is active; finish it before restarting the server'}
}
$taskBackup=Join-Path $taskWorkspace ('output\ui-backup-'+(Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $taskBackup | Out-Null
if (Test-Path -LiteralPath (Join-Path $taskInstalled 'index.html')) {Copy-Item -LiteralPath (Join-Path $taskInstalled 'index.html') -Destination $taskBackup}
if (Test-Path -LiteralPath (Join-Path $taskInstalled 'ui')) {Copy-Item -LiteralPath (Join-Path $taskInstalled 'ui') -Destination (Join-Path $taskBackup 'ui') -Recurse}
$taskStopped=$false
try {
    if ($RestartServer -and $taskRunning.Count) {
        Invoke-RestMethod -Uri 'http://127.0.0.1:4242/api/system/shutdown' -Method Post -TimeoutSec 5 | Out-Null
        for ($taskAttempt=0; $taskAttempt -lt 40; $taskAttempt++) {
            if (-not @(Get-CimInstance Win32_Process -Filter "Name='swch-server.exe'" | Where-Object {$_.ExecutablePath -eq $taskBackend}).Count) {$taskStopped=$true;break}
            Start-Sleep -Milliseconds 250
        }
        if (-not $taskStopped) {throw 'Installed server did not stop; UI has not been replaced'}
    }
    Copy-Item -LiteralPath $taskIndex -Destination (Join-Path $taskInstalled 'index.html') -Force
    New-Item -ItemType Directory -Force -Path (Join-Path $taskInstalled 'ui') | Out-Null
    Get-ChildItem -LiteralPath $taskUi | ForEach-Object {Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $taskInstalled 'ui') -Recurse -Force}
    $taskFiles=@(Get-Item -LiteralPath $taskIndex)+@(Get-ChildItem -LiteralPath $taskUi -File -Recurse)
    foreach ($taskFile in $taskFiles) {
        $taskRelative=$taskFile.FullName.Substring($taskWorkspace.Length+1)
        $taskTarget=Join-Path $taskInstalled $taskRelative
        if ((Get-FileHash -LiteralPath $taskFile.FullName).Hash -ne (Get-FileHash -LiteralPath $taskTarget).Hash) {throw "UI hash mismatch: $taskRelative"}
    }
    if ($taskStopped) {
        Start-Process -FilePath $taskBackend -WorkingDirectory $taskInstalled -WindowStyle Hidden
        $taskReady=$false
        for ($taskAttempt=0; $taskAttempt -lt 40; $taskAttempt++) {
            try {Invoke-RestMethod -Uri 'http://127.0.0.1:4242/api/health' -TimeoutSec 1 | Out-Null;$taskReady=$true;break} catch {Start-Sleep -Milliseconds 250}
        }
        if (-not $taskReady) {throw 'Installed server did not become healthy'}
    }
    [pscustomobject]@{Updated=$true;Backup=$taskBackup;VerifiedFiles=$taskFiles.Count;ServerRestarted=$taskStopped} | ConvertTo-Json -Compress
} catch {
    if (Test-Path -LiteralPath (Join-Path $taskBackup 'index.html')) {Copy-Item -LiteralPath (Join-Path $taskBackup 'index.html') -Destination (Join-Path $taskInstalled 'index.html') -Force}
    if (Test-Path -LiteralPath (Join-Path $taskBackup 'ui')) {Get-ChildItem -LiteralPath (Join-Path $taskBackup 'ui') | ForEach-Object {Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $taskInstalled 'ui') -Recurse -Force}}
    if ($taskStopped -and -not @(Get-CimInstance Win32_Process -Filter "Name='swch-server.exe'" | Where-Object {$_.ExecutablePath -eq $taskBackend}).Count) {Start-Process -FilePath $taskBackend -WorkingDirectory $taskInstalled -WindowStyle Hidden}
    throw
}
