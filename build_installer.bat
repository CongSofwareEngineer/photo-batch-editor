@echo off
rem Builds installer_output\PhotoBatchEditor-Setup-<version>.exe from dist\PhotoBatchEditor.
rem Run build_windows.bat first. Needs Inno Setup 6 (winget install JRSoftware.InnoSetup).
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
"%ISCC%" installer.iss || exit /b 1
echo.
echo Installer: installer_output\
endlocal
