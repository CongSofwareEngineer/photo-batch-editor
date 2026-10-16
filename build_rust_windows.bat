@echo off
REM Build Rust version for Windows: tang version -> exe (icon + version) -> ky so -> bo cai dat (Inno Setup, ky so)
REM Ky bang chung chi mua: set PBE_SIGN_PFX=...\cert.pfx va PBE_SIGN_PFX_PASSWORD=... truoc khi chay.
setlocal
cd /d "%~dp0"
set "PS=powershell -NoProfile -ExecutionPolicy Bypass -File"
echo Building Photo Batch Editor (Rust) for Windows...

if not exist ffmpeg\ffmpeg.exe (
  echo [LOI] Thieu ffmpeg\ffmpeg.exe - can cho Video editor. Tai ffmpeg roi dat vao thu muc ffmpeg\.
  goto :fail
)

REM Tim trinh bien dich Inno Setup
set "ISCC="
for %%P in ("%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe" "%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe" "%ProgramFiles%\Inno Setup 6\ISCC.exe") do (
  if exist %%P set "ISCC=%%~P"
)
if not defined ISCC (
  echo [LOI] Chua cai Inno Setup 6: winget install JRSoftware.InnoSetup
  goto :fail
)

REM 1. Moi lan build = 1 version moi (tang "build" trong version.json)
set "APPVER="
for /f "usebackq delims=" %%V in (`%PS% tools\bump_version.ps1`) do set "APPVER=%%V"
if not defined APPVER goto :fail
echo Version: %APPVER%

REM 2. Build exe (build.rs nhung icon + version vao exe)
cd rust
cargo build -p pbe-gui --release
if errorlevel 1 (
  cd ..
  goto :fail
)
cd ..

REM 3. Ky so exe (truoc khi dong goi vao Setup)
%PS% tools\sign_windows.ps1 -Files rust\target\release\photo-batch-editor.exe
if errorlevel 1 goto :fail

REM Tai nguyen canh exe (chay thu exe truc tiep): app tim models\, presets_builtin\, ffmpeg\, version.json ben canh exe
xcopy /E /I /Y models rust\target\release\models >nul
xcopy /E /I /Y presets_builtin rust\target\release\presets_builtin >nul
xcopy /E /I /Y ffmpeg rust\target\release\ffmpeg >nul
copy /Y version.json rust\target\release\version.json >nul

REM 4. Bo cai dat; Inno goi sign_windows.ps1 de ky Setup + uninstaller ($f = file can ky)
"%ISCC%" /Q /DAppVersion=%APPVER% /DSignTool "/Spbesign=powershell.exe -NoProfile -ExecutionPolicy Bypass -File $q%CD%\tools\sign_windows.ps1$q -Files $f" installer.iss
if errorlevel 1 goto :fail

REM 5. Kem chung chi + script cai chung chi cho may khac
%PS% tools\sign_windows.ps1 -ExportCertTo installer_output
if errorlevel 1 goto :fail
copy /Y tools\CaiChungChi.bat installer_output\CaiChungChi.bat >nul

echo.
echo Build complete (version %APPVER%):
echo   rust\target\release\photo-batch-editor.exe
echo   installer_output\PhotoBatchEditor-Setup-%APPVER%.exe   ^<-- file cai dat de phat hanh
echo   installer_output\PhotoBatchEditor.cer + CaiChungChi.bat ^<-- gui kem neu ky bang chung chi tu ky
pause
exit /b 0

:fail
echo Build FAILED.
pause
exit /b 1
