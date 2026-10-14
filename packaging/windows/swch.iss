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
UninstallDisplayName=SWCH
CreateUninstallRegKey=yes
Uninstallable=yes
VersionInfoVersion={#NumericVersion}
AppPublisher=Kabarab
AppPublisherURL=https://github.com/Kabarab/swch
AppSupportURL=https://github.com/Kabarab/swch/issues
DefaultDirName={code:DefaultSwchDirectory}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
UsePreviousPrivileges=no
ArchitecturesAllowed=x64os
ArchitecturesInstallIn64BitMode=x64os
MinVersion=10.0
OutputDir={#InstallerOutput}
OutputBaseFilename=SWCH-Setup-{#AppVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SetupIconFile=..\..\src\assets\swch.ico
UninstallDisplayIcon={app}\swch-logo-link.ico
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
Name: "{autoprograms}\SWCH"; Filename: "{app}\swch.exe"; WorkingDir: "{app}"; IconFilename: "{app}\swch-logo-link.ico"; AppUserModelID: "com.kabarab.swch"
Name: "{autodesktop}\SWCH"; Filename: "{app}\swch.exe"; WorkingDir: "{app}"; IconFilename: "{app}\swch-logo-link.ico"; AppUserModelID: "com.kabarab.swch"; Tasks: desktopicon

[Run]
Filename: "{app}\swch.exe"; Description: "{cm:LaunchProgram,SWCH}"; WorkingDir: "{app}"; Flags: nowait postinstall skipifsilent

; Keep databases, settings and mod data. Only packaged files are uninstalled.

[Code]
const
  SwchUninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{71E78E5E-0A60-4F4F-A637-8040CCDB9154}_is1';

function RegisteredSwchDirectory(Root: Integer): String;
var
  Directory: String;
begin
  Result := '';
  if RegQueryStringValue(Root, SwchUninstallKey, 'InstallLocation', Directory) and
     FileExists(AddBackslash(Directory) + 'swch.exe') and
     FileExists(AddBackslash(Directory) + 'unins000.exe') then
    Result := Directory;
end;

function DefaultSwchDirectory(Param: String): String;
begin
  { Keep an existing installation when switching from user to machine scope. }
  Result := '';
  if IsAdminInstallMode then
    Result := RegisteredSwchDirectory(HKLM64);
  if Result = '' then
    Result := RegisteredSwchDirectory(HKCU64);
  if Result = '' then
    Result := ExpandConstant('{autopf}\SWCH');
end;

function SendMessageTimeout(Window: LongWord; Msg: LongWord; WParam: LongWord;
  LParam: String; Flags: LongWord; Timeout: LongWord; var ResultValue: LongWord): LongWord;
  external 'SendMessageTimeoutW@user32.dll stdcall';

procedure CurStepChanged(CurStep: TSetupStep);
var
  UninstallKey: String;
  Root: Integer;
  ResultValue: LongWord;
  UserDirectory: String;
  UserUninstall: String;
  MachineUninstall: String;
begin
  if CurStep <> ssPostInstall then
    Exit;

  UninstallKey := SwchUninstallKey;
  if IsAdminInstallMode then
    Root := HKLM32
  else
    Root := HKCU32;
  if Is64BitInstallMode then
  begin
    if IsAdminInstallMode then
      Root := HKLM64
    else
      Root := HKCU64;
  end;

  { Repair visibility flags left by an earlier installation. }
  if not RegWriteDWordValue(Root, UninstallKey, 'SystemComponent', 0) or
     not RegWriteDWordValue(Root, UninstallKey, 'NoRemove', 0) then
    RaiseException('Could not register SWCH as a visible installed application.');

  { A scope migration in the same folder uses one uninstaller, not two apps. }
  if IsAdminInstallMode and
     RegQueryStringValue(HKCU64, UninstallKey, 'InstallLocation', UserDirectory) and
     (CompareText(AddBackslash(UserDirectory), AddBackslash(ExpandConstant('{app}'))) = 0) and
     RegQueryStringValue(HKCU64, UninstallKey, 'UninstallString', UserUninstall) and
     RegQueryStringValue(Root, UninstallKey, 'UninstallString', MachineUninstall) and
     (CompareText(UserUninstall, MachineUninstall) = 0) then
  begin
    if not RegDeleteKeyIncludingSubkeys(HKCU64, UninstallKey) then
      RaiseException('Could not remove the previous SWCH user registration.');
    if not RegWriteStringValue(Root, UninstallKey, 'DisplayName', 'SWCH') then
      RaiseException('Could not update the SWCH application name.');
    Log('Migrated the same SWCH installation from user to machine registration.');
  end;

  { Notify open Windows settings windows after the uninstall entry is written. }
  SendMessageTimeout($FFFF, $001A, 0,
    'Software\Microsoft\Windows\CurrentVersion\Uninstall', 2, 1000, ResultValue);
end;
