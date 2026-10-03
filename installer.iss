; Inno Setup script — Windows installer for Photo Batch Editor.
; Build: build_installer.bat  (needs Inno Setup 6 and dist\PhotoBatchEditor from build_windows.bat)

#define AppName "Photo Batch Editor"
#define AppVersion "1.0.0"
#define AppExe "PhotoBatchEditor.exe"

[Setup]
AppId={{FA6B7FF5-556D-496A-AB0F-B1598CD6FCD8}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Photo Batch Editor
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
; Allows installing for the current user only (no admin rights needed) or for all users
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=installer_output
OutputBaseFilename=PhotoBatchEditor-Setup-{#AppVersion}
SetupIconFile=assets\app.ico
UninstallDisplayIcon={app}\{#AppExe}
WizardStyle=modern
; ~3 GB of files (mostly the CUDA/cuDNN libraries for GPU mode)
Compression=lzma2/normal
SolidCompression=no
LZMAUseSeparateProcess=yes
LZMANumBlockThreads=4
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "dist\PhotoBatchEditor\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autoprograms}\{#AppName} - GPU check"; Filename: "{app}\{#AppExe}"; \
    Parameters: "--selftest ""{userdocs}\PhotoBatchEditor-selftest.json"""; \
    Comment: "Checks the NVIDIA GPU and writes Documents\PhotoBatchEditor-selftest.json"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

; User presets and settings in %APPDATA%\PhotoBatchEditor are kept on uninstall.
[UninstallDelete]
Type: filesandordirs; Name: "{localappdata}\PhotoBatchEditor\cupy_cache"
