@echo off
REM Build Rust version for Windows
echo Building Photo Batch Editor (Rust) for Windows...

cd rust
cargo build -p pbe-gui --release

REM Models (Super Resolution, tach nen MODNet) canh file exe: app tim models\ ben canh exe
xcopy /E /I /Y ..\models target\release\models >nul

echo Build complete:
echo   rust\target\release\photo-batch-editor.exe
echo   rust\target\release\models\  (copy cung exe khi phat hanh)
pause
