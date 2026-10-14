[CmdletBinding()]
param(
    [string]$PackageDirectory = 'package',
    [string]$Version,
    [string]$OutputDirectory = 'output/installers',
    [string]$IsccPath
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
if (-not $Version) {
    $cargo = Get-Content -LiteralPath (Join-Path $projectRoot 'Cargo.toml') -Raw
    $Version = [regex]::Match($cargo, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
}
if ($Version -notmatch '^(\d+\.\d+\.\d+)(?:-[A-Za-z0-9.-]+)?$') { throw 'Invalid release version' }
$numericVersion = $Matches[1]
$packagePath = (Resolve-Path -LiteralPath $PackageDirectory).Path
foreach ($required in @('swch.exe', 'swch-server.exe', 'swch-logo-link.ico', 'runtime/node.exe', 'index.html', 'ui/launcher.js', 'ui/mod-guide.js')) {
    if (-not (Test-Path -LiteralPath (Join-Path $packagePath $required) -PathType Leaf)) {
        throw "Incomplete Windows package: $required"
    }
}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$outputPath = (Resolve-Path -LiteralPath $OutputDirectory).Path
if (-not $IsccPath) {
    $toolRoot = Join-Path $projectRoot 'output/installer-tools'
    $compilerDirectory = Join-Path $toolRoot 'inno'
    $IsccPath = Join-Path $compilerDirectory 'ISCC.exe'
    if (-not (Test-Path -LiteralPath $IsccPath)) {
        New-Item -ItemType Directory -Force -Path $toolRoot | Out-Null
        $compilerSetup = Join-Path $toolRoot 'innosetup-6.7.3.exe'
        Invoke-WebRequest 'https://github.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6.7.3.exe' -OutFile $compilerSetup
        $expected = '9c73c3bae7ed48d44112a0f48e66742c00090bdb5bef71d9d3c056c66e97b732'
        if ((Get-FileHash -LiteralPath $compilerSetup -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected) {
            throw 'Inno Setup compiler download checksum mismatch'
        }
        $signature = Get-AuthenticodeSignature -LiteralPath $compilerSetup
        if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'Pyrsys') {
            throw 'Inno Setup compiler download signature is invalid'
        }
        $arguments = @('/PORTABLE=1', '/CURRENTUSER', '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/NOICONS', '/TASKS=', ('"/DIR=' + $compilerDirectory + '"'))
        $process = Start-Process -FilePath $compilerSetup -ArgumentList $arguments -WindowStyle Hidden -Wait -PassThru
        if ($process.ExitCode -ne 0) { throw 'Portable Inno Setup compiler extraction failed' }
    }
}
$IsccPath = (Resolve-Path -LiteralPath $IsccPath).Path
$signature = Get-AuthenticodeSignature -LiteralPath $IsccPath
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'Pyrsys') {
    throw 'Inno Setup compiler signature is invalid'
}
# Include every translation shipped by the verified compiler, with English fallback.
$languageInclude = Join-Path $outputPath 'installer-languages.issinc'
$languages = @('Name: "english"; MessagesFile: "compiler:Default.isl"')
foreach ($language in Get-ChildItem -LiteralPath (Join-Path (Split-Path -Parent $IsccPath) 'Languages') -Filter '*.isl' | Sort-Object Name) {
    $name = ($language.BaseName -replace '[^A-Za-z0-9_]', '_').ToLowerInvariant()
    if ($name -ne 'english') { $languages += ('Name: "' + $name + '"; MessagesFile: "compiler:Languages\' + $language.Name + '"') }
}
Set-Content -LiteralPath $languageInclude -Value $languages -Encoding utf8
$script = Join-Path $projectRoot 'packaging/windows/swch.iss'
& $IsccPath '/Qp' "/DAppVersion=$Version" "/DNumericVersion=$numericVersion" "/DPackageDirectory=$packagePath" "/DInstallerOutput=$outputPath" "/DLanguageInclude=$languageInclude" $script
if ($LASTEXITCODE -ne 0) { throw 'Windows installer compilation failed' }
$installer = Join-Path $outputPath "SWCH-Setup-$Version.exe"
if (-not (Test-Path -LiteralPath $installer -PathType Leaf)) { throw 'Installer output is missing' }
Write-Host "Built $installer ($($languages.Count) installer languages)"
Get-FileHash -LiteralPath $installer -Algorithm SHA256
