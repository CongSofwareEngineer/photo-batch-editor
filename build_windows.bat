@echo off
rem PRODUCTION BUILD FOR WINDOWS 10 / 11 (64-bit) - run on Windows.
rem   tests -> bootloader -> build number +1 -> dist\PhotoBatchEditor\PhotoBatchEditor.exe -> installer (if Inno Setup 6 is installed)
rem   build_windows.bat               full build
rem   build_windows.bat --skip-tests  skip pytest
rem One build runs on both Windows 10 and Windows 11. For a Mac build use build_mac.sh on a Mac.
rem For day-to-day work use dev.bat instead (live reload, no build).
setlocal
cd /d "%~dp0"

if not exist venv\Scripts\python.exe (
    python -m venv venv || exit /b 1
    venv\Scripts\python -m pip install --upgrade pip
    venv\Scripts\pip install -r requirements-dev.txt || exit /b 1
)
set "PY=venv\Scripts\python.exe"

if /i not "%~1"=="--skip-tests" (
    echo === Lint + type check ===
    call "%~dp0lint.bat" --check || exit /b 1
    echo === Tests ===
    %PY% -m pytest -q || exit /b 1
)

rem The .spec file is the PyInstaller command of the spec plus the CUDA DLLs and hidden
rem imports needed by CuPy and ONNX Runtime (section 10.6):
rem   pyinstaller --noconfirm --onedir --windowed --name PhotoBatchEditor ^
rem     --add-data "ui/theme.qss;ui" --add-data "presets_builtin;presets_builtin" ^
rem     --add-data "models;models" main.py
echo === FFmpeg (Video editor) ===
%PY% tools/fetch_ffmpeg.py || exit /b 1

echo === Bootloader (built from source: fewer false antivirus alerts) ===
call "%~dp0tools\build_bootloader.bat" || echo WARNING: using the prebuilt PyInstaller bootloader - Windows Defender may flag the .exe.

echo === Version ===
for /f "delims=" %%V in ('%PY% tools/bump_build.py') do set "APP_VERSION=%%V"
if not defined APP_VERSION exit /b 1
echo Version %APP_VERSION%

echo === PyInstaller ===
%PY% -m PyInstaller --noconfirm PhotoBatchEditor.spec || exit /b 1
echo Built: dist\PhotoBatchEditor\PhotoBatchEditor.exe  (version %APP_VERSION%)

echo === Installer ===
call "%~dp0build_installer.bat" || echo Installer skipped - the .exe above is ready to use.
endlocal
