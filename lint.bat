@echo off
rem Like "eslint": format + lint + type check all Python code (rules: pyproject.toml).
rem VS Code shows the same problems while typing and formats + fixes on save.
rem   lint.bat          fix: format files, auto-fix lint problems, then type check
rem   lint.bat --check  only check (no changes), exit code 1 if something is wrong
setlocal
cd /d "%~dp0"
set "PY=venv\Scripts\python.exe"
if /i "%~1"=="--check" (
    %PY% -m ruff format --check . || exit /b 1
    %PY% -m ruff check . || exit /b 1
) else (
    %PY% -m ruff format . || exit /b 1
    %PY% -m ruff check --fix . || exit /b 1
)
echo === Type check (Pyright) ===
%PY% -m pyright || exit /b 1
endlocal
