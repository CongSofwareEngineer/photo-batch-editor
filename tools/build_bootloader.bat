@echo off
rem Rebuilds the PyInstaller bootloader from source in venv\ (Windows only, free).
rem Why: the prebuilt bootloader shipped on PyPI is byte-identical in every PyInstaller app,
rem malware included, so Windows Defender often flags it (Trojan:Win32/Wacatac...). A bootloader
rem compiled on this machine has its own bytes and is flagged far less often.
rem
rem Needs the Microsoft C++ Build Tools (free), once:
rem   winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
rem
rem   tools\build_bootloader.bat          build if not done yet for this PyInstaller version
rem   tools\build_bootloader.bat --force  build again
rem Called by build_windows.bat. Marker: venv\custom_bootloader.txt (PyInstaller version).
setlocal
cd /d "%~dp0.."
set "PY=venv\Scripts\python.exe"
if not exist %PY% (
    echo venv not found - run build_windows.bat first.
    exit /b 1
)

for /f "delims=" %%V in ('%PY% -c "import PyInstaller; print(PyInstaller.__version__)"') do set "PI_VER=%%V"
if not defined PI_VER (
    echo PyInstaller is not installed in venv.
    exit /b 1
)

set "MARKER=venv\custom_bootloader.txt"
if /i not "%~1"=="--force" if exist %MARKER% (
    findstr /x /c:"%PI_VER%" %MARKER% >nul && (
        echo Custom bootloader already built for PyInstaller %PI_VER%.
        exit /b 0
    )
)

set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
set "VCTOOLS="
if exist "%VSWHERE%" (
    for /f "delims=" %%I in ('"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath') do set "VCTOOLS=%%I"
)
if not defined VCTOOLS (
    echo Microsoft C++ Build Tools not found - cannot compile the bootloader. Install them with:
    echo   winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
    exit /b 1
)

echo Compiling the PyInstaller %PI_VER% bootloader from source (a few minutes)...
set "PYINSTALLER_COMPILE_BOOTLOADER=1"
%PY% -m pip install --force-reinstall --no-deps --no-cache-dir --no-binary pyinstaller "pyinstaller==%PI_VER%" || (
    echo Bootloader build failed - reinstalling the prebuilt PyInstaller.
    set "PYINSTALLER_COMPILE_BOOTLOADER="
    %PY% -m pip install --force-reinstall --no-deps "pyinstaller==%PI_VER%"
    exit /b 1
)
> %MARKER% echo %PI_VER%
echo Custom bootloader built for PyInstaller %PI_VER%.
endlocal
