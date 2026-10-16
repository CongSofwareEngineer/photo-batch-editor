; Installer Windows cho Photo Batch Editor (Inno Setup 6). Chay qua build_rust_windows.bat.
; Duong dan Source tinh tu thu muc chua file nay (goc du an).

; AppVersion = X.Y.Z.build (tools\bump_version.ps1 tang build moi lan build).
; /DSignTool + /Spbesign=... (tu build_rust_windows.bat): ky Setup va uninstaller bang tools\sign_windows.ps1.
#ifndef AppVersion
  #define AppVersion "1.0.0.0"
#endif
#define AppName "Photo Batch Editor"
#define AppExe "photo-batch-editor.exe"
#define ReleaseDir "rust\target\release"

[Setup]
; AppId co dinh: doi la Windows coi la app khac, khong nang cap duoc ban cu.
AppId={{DC4CD855-836D-43DC-914D-3CB84E7664A5}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppName}
VersionInfoVersion={#AppVersion}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=installer_output
OutputBaseFilename=PhotoBatchEditor-Setup-{#AppVersion}
SetupIconFile=assets\app.ico
UninstallDisplayIcon={app}\app.ico
UninstallDisplayName={#AppName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
#ifdef SignTool
SignTool=pbesign
SignedUninstaller=yes
#endif

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#ReleaseDir}\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "version.json"; DestDir: "{app}"; Flags: ignoreversion
Source: "assets\app.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "presets_builtin\*"; DestDir: "{app}\presets_builtin"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "models\*"; DestDir: "{app}\models"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "ffmpeg\ffmpeg.exe"; DestDir: "{app}\ffmpeg"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; IconFilename: "{app}\app.ico"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; IconFilename: "{app}\app.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
