; Gesso Windows installer (Inno Setup 6).
; Per-user install: no admin required, installs to {localappdata}\Programs\Gesso.
; Version/arch are injected by build_portable.ps1 via /DAppVersion /DAppArch.
; UI strings stay ASCII on purpose: .iss without a UTF-8 BOM is read as ANSI,
; non-ASCII literals would garble.

#define AppName "Gesso"
#ifndef AppVersion
#define AppVersion "0.0.0"
#endif
#ifndef AppArch
#define AppArch "x64"
#endif

[Setup]
AppId={{6B9F7A64-53C1-4E8B-9A2D-1F0C3E5D7B21}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=Gesso
; lowest = per-user: {autopf} resolves to {userpf} ({localappdata}\Programs)
PrivilegesRequired=lowest
DefaultDirName={userpf}\{#AppName}
OutputDir=target\release-bundle
OutputBaseFilename=Gesso-{#AppVersion}-{#AppArch}-setup
Compression=lzma2
SolidCompression=yes
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\gesso.exe
DisableProgramGroupPage=yes

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; Flags: unchecked

[Files]
Source: "target\release\gesso.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "crates\app\assets\host\*"; DestDir: "{app}\assets\host"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "crates\app\assets\samples\*"; DestDir: "{app}\assets\samples"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{userprograms}\{#AppName}"; Filename: "{app}\gesso.exe"
Name: "{userdesktop}\{#AppName}"; Filename: "{app}\gesso.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\gesso.exe"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Nothing: user data lives in %USERPROFILE%\.gesso and is intentionally kept.
