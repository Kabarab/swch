; Compile with scripts/build-windows-installer.ps1.
#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef PackageDirectory
  #error PackageDirectory is required
#endif
#ifndef InstallerOutput
  #error InstallerOutput is required
#endif
#ifndef LanguageInclude
  #error LanguageInclude is required
#endif

[Setup]
AppId={{71E78E5E-0A60-4F4F-A637-8040CCDB9154}
AppName=SWCH
AppVersion={#AppVersion}
VersionInfoVersion={#NumericVersion}
AppPublisher=Kabarab
AppPublisherURL=https://github.com/Kabarab/swch
AppSupportURL=https://github.com/Kabarab/swch/issues
DefaultDirName={localappdata}\Programs\SWCH
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64os
ArchitecturesInstallIn64BitMode=x64os
MinVersion=10.0
OutputDir={#InstallerOutput}
OutputBaseFilename=SWCH-Setup-{#AppVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SetupIconFile=..\..\src\assets\swch.ico
UninstallDisplayIcon={app}\swch.exe
CloseApplications=yes
CloseApplicationsFilter=swch.exe,swch-server.exe,node.exe
RestartApplications=no
SetupLogging=yes

[Languages]
#include LanguageInclude

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#PackageDirectory}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\SWCH"; Filename: "{app}\swch.exe"; WorkingDir: "{app}"; AppUserModelID: "com.kabarab.swch"
Name: "{autodesktop}\SWCH"; Filename: "{app}\swch.exe"; WorkingDir: "{app}"; AppUserModelID: "com.kabarab.swch"; Tasks: desktopicon

[Run]
Filename: "{app}\swch.exe"; Description: "{cm:LaunchProgram,SWCH}"; WorkingDir: "{app}"; Flags: nowait postinstall skipifsilent

; Keep databases, settings and mod data. Only packaged files are uninstalled.
