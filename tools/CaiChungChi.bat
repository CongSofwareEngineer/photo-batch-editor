@echo off
REM Chay 1 LAN tren may cai app (dat cung thu muc voi PhotoBatchEditor.cer va file Setup):
REM  - cai chung chi Photo Batch Editor vao Trusted Root + Trusted Publishers (ca may)
REM  - go danh dau "tai tu Internet" (Mark-of-the-Web) khoi Setup -> SmartScreen khong hoi "Run anyway"
setlocal
cd /d "%~dp0"

net session >nul 2>&1
if errorlevel 1 (
  echo Can quyen Administrator - dang mo lai...
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
  exit /b
)

if not exist PhotoBatchEditor.cer (
  echo [LOI] Khong thay PhotoBatchEditor.cer canh file nay.
  pause
  exit /b 1
)

certutil -f -addstore Root PhotoBatchEditor.cer >nul || goto :fail
certutil -f -addstore TrustedPublisher PhotoBatchEditor.cer >nul || goto :fail
powershell -NoProfile -Command "Get-ChildItem -Path '%~dp0' -Filter 'PhotoBatchEditor-Setup-*.exe' | Unblock-File"

echo Da cai chung chi Photo Batch Editor. Gio chay file PhotoBatchEditor-Setup-*.exe de cai app.
pause
exit /b 0

:fail
echo [LOI] Khong cai duoc chung chi.
pause
exit /b 1
