; Inno Setup script for the Windows installer.
; Built by GitHub Actions:  iscc /DAppVersion=0.4.0 /DArch=x64 /DTarget=x86_64-pc-windows-msvc installer\windows\busmanager.iss
; Expects target\<Target>\release\busmanager.exe to exist (cargo build --release).

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
; Arch is "x64" or "arm64"; Target is the Rust target the exe was built for.
#ifndef Arch
  #define Arch "x64"
#endif
#ifndef Target
  #define Target "x86_64-pc-windows-msvc"
#endif

[Setup]
AppId={{6F0B2D5E-3C1A-4B7E-9D52-0B05A0A9E001}
AppName=Bus Manager
AppVersion={#AppVersion}
AppPublisher=YOZ Team
AppPublisherURL=https://busmanager.yoz-team.online
AppSupportURL=https://github.com/YOZTeam/BusManager/issues
AppUpdatesURL=https://github.com/YOZTeam/BusManager/releases
DefaultDirName={autopf}\Bus Manager
DefaultGroupName=Bus Manager
DisableProgramGroupPage=yes
UninstallDisplayIcon={app}\busmanager.exe
UninstallDisplayName=Bus Manager
SetupIconFile=..\..\game\assets\icon.ico
OutputDir=..\..\dist
OutputBaseFilename=BusManager-Setup-windows-{#Arch}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
#if Arch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif
; Per-user install by default (no admin prompt); the user may choose "all users".
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\..\target\{#Target}\release\busmanager.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Bus Manager"; Filename: "{app}\busmanager.exe"
Name: "{group}\Uninstall Bus Manager"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Bus Manager"; Filename: "{app}\busmanager.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\busmanager.exe"; Description: "{cm:LaunchProgram,Bus Manager}"; Flags: nowait postinstall skipifsilent

; Saves, settings and downloaded maps live in the user's data folder and are kept on uninstall.
