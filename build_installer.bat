@echo off
rem Builds installer_output\PhotoBatchEditor-Setup-<version>.exe (version from version.json) from dist\PhotoBatchEditor.
rem Run build_windows.bat first. Needs Inno Setup 6 (winget install JRSoftware.InnoSetup).
rem Signs the app .exe and the installer (tools\sign_windows.ps1) and puts PhotoBatchEditor.cer
rem next to them: install that .cer once on the target PC, then nothing warns anymore.
setlocal
cd /d "%~dp0"
set "ISCC="
for %%P in ("%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe" "%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe" "%ProgramFiles%\Inno Setup 6\ISCC.exe") do (
    if exist %%P set "ISCC=%%~P"
)
if not defined ISCC (
    echo Inno Setup 6 not found. Install it with: winget install JRSoftware.InnoSetup
    exit /b 1
)
if not exist dist\PhotoBatchEditor\PhotoBatchEditor.exe (
    echo dist\PhotoBatchEditor not found - run build_windows.bat first.
    exit /b 1
)
set "APP_VERSION="
for /f "delims=" %%V in ('venv\Scripts\python.exe tools/bump_build.py --print') do set "APP_VERSION=%%V"
if not defined APP_VERSION (
    echo Could not read version.json - run build_windows.bat first.
    exit /b 1
)
echo === Sign the app .exe (so the installer packs a signed file) ===
powershell -NoProfile -ExecutionPolicy Bypass -File "tools\sign_windows.ps1" -Files "dist\PhotoBatchEditor\PhotoBatchEditor.exe" -CopyCertificateTo "installer_output" || exit /b 1
"%ISCC%" /DAppVersion=%APP_VERSION% installer.iss || exit /b 1
echo === Sign the installer ===
powershell -NoProfile -ExecutionPolicy Bypass -File "tools\sign_windows.ps1" -Files "installer_output\PhotoBatchEditor-Setup-%APP_VERSION%.exe" || exit /b 1
echo.
echo Installer: installer_output\  (PhotoBatchEditor-Setup-%APP_VERSION%.exe + PhotoBatchEditor.cer)
endlocal
