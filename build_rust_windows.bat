@echo off
REM Build Rust version for Windows
echo Building Photo Batch Editor (Rust) for Windows...

cd rust
cargo build -p pbe-gui --release

echo Build complete:
echo   rust\target\release\photo-batch-editor.exe
pause
