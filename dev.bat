@echo off
rem DEV: runs the app from source with live reload (dev.py) - no build.
rem Save a .py file -> the app restarts on the same page; save ui\theme.qss -> applied instantly.
setlocal
cd /d "%~dp0"
if not exist venv\Scripts\python.exe (
    echo venv\ not found. Create it once with:
    echo   python -m venv venv ^&^& venv\Scripts\pip install -r requirements-dev.txt
    exit /b 1
)
venv\Scripts\python.exe dev.py %*
endlocal
