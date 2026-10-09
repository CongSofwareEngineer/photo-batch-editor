# Photo Batch Editor

Desktop app (Windows 10/11 64-bit and macOS 11+ — Apple Silicon or Intel MacBook) that applies the same Camera Raw–style look to a whole
folder of photos: choose a folder → pick a preset or set the adjustments → **RUN** → the
edited photos are written as JPEG quality 100 (4:4:4) to the sibling folder
`<folder>_update`, with the same file names and sub-folders. Originals are never modified.

* 15 adjustments named like Camera Raw (Temperature, Tint, Exposure, Brightness, Contrast,
  Highlights, Shadows, Whites, Blacks, Clarity, Vibrance, Saturation, Sharpening › Amount,
  Noise Reduction, Post-Crop Vignetting › Amount) + **Super Resolution** (Real-ESRGAN, 2x/4x)
  + **Image Size** (Photoshop resample methods).
* NVIDIA GPU (CUDA 12, CuPy + ONNX Runtime) by default, automatic fallback to the CPU.
  The status chip always shows the device in use and, if the GPU cannot be used, why.
* Live preview, built-in and user presets, results list with a large before/after viewer.
* **Sign-in** on first start (default account `admin` / `admin`, change it in
  Settings › Account). With “Keep me signed in” the app opens directly next time;
  “Sign out” in the sidebar asks for the password again.
* **Settings page** (sidebar, Ctrl+4): create as many saved filter settings as you like
  (blank, from the Editor's current adjustments, from a built-in template, or imported from
  a `.json` file), edit them with a live preview on a sample photo, rename, duplicate,
  export, delete. Every change is saved automatically; pick a setting in the Editor's
  “Setting” box (or “Use in Editor”) to apply it.

* **Photo editor** (sidebar, Ctrl+2) — one photo, Photoshop-style, with layers: text
  (any font, size, colour, bold/italic, outline, opacity; Vietnamese accents), inserted images
  (PNG transparency, move / resize with Shift / rotate), blur or pixelate areas (rectangle,
  ellipse or brush), crop (free, original, 1:1, 4:3, 16:9, 9:16), rotate 90° / straighten / flip,
  brightness / contrast / saturation / temperature / sharpness (same algorithms as the batch),
  collage templates. Ctrl + wheel zooms at the cursor (10–1600 %), Space + drag pans, Ctrl+0 fit,
  Ctrl+1 100 %, Ctrl+Z / Ctrl+Y (50 steps). Export JPEG (quality) or PNG to `<photo folder>\editor`;
  save a `.pbep` project to keep the layers editable. Right-click a photo in the batch editor
  (input list or results) › *Open in Photo editor*.
* **Video editor** (sidebar, Ctrl+3) — preview, timeline, split / reorder / delete clips,
  In / Out marks (keep or delete a range), mute, texts with start / end times (drag them on the
  preview), music (start offset, own volume, fade out, cut at the end of the video). Export MP4 or
  MOV, original / 1080p / 720p, in the background with a progress bar and Cancel, to
  `<video folder>\editor` (the original is never overwritten). Uses FFmpeg, bundled with the app
  (`ffmpeg\ffmpeg.exe`); from source it comes from the `imageio-ffmpeg` package.

* **English / Tiếng Việt**: choose the language in Settings › General (or on the sign-in
  page). The window is rebuilt immediately — folder, adjustments and settings are kept. The
  first start follows the Windows display language. Texts live in `core/i18n_vi.py`
  (English text = key); `tests/test_i18n.py` fails if a new `tr("…")` text has no
  Vietnamese entry.

## Requirements

* Windows 10/11 64-bit, or macOS 11 Big Sur or newer (Apple Silicon M1–M4 or Intel).
  Linux works for development/tests.
* Python 3.11 or 3.12 to run from source.
* macOS has no NVIDIA GPU: the app always runs on the CPU there (all features work, Super
  Resolution is slower). `pip` installs the right packages per OS automatically
  (`cupy-cuda12x` / `onnxruntime-gpu` on Windows, `onnxruntime` on the Mac).
* GPU acceleration: an NVIDIA GPU with a driver that supports **CUDA 12** (driver ≥ 525;
  update via the NVIDIA App or nvidia.com). The CUDA Toolkit is **not** needed: the CUDA
  runtime libraries are installed as pip packages (`requirements-cuda.txt`) and bundled
  with the `.exe`. Without an NVIDIA GPU everything runs on the CPU.

## Install and run from source

Windows:

```bat
python -m venv venv
venv\Scripts\activate
pip install -r requirements-dev.txt
python main.py
```

macOS (Terminal; Python from `brew install python@3.12` or python.org):

```bash
python3.12 -m venv venv
source venv/bin/activate
pip install -r requirements-dev.txt
python main.py
```

`requirements.txt` holds the app libraries, `requirements-cuda.txt` adds the CUDA 12 runtime
libraries (≈ 2.5 GB, only useful with an NVIDIA GPU), `requirements-dev.txt` adds pytest and
PyInstaller. `onnxruntime-gpu` is pinned below 1.27 because newer releases are built for
CUDA 13; never install the plain `onnxruntime` package next to it.

Command-line batch (no UI):

```bat
python -m core.batch "D:\Shoot\clientA" --preset presets_builtin\warm.json --device gpu
python -m core.batch "D:\Shoot\clientA" --device cpu --super-resolution 2x --if-exists overwrite
```

## Tests

```bat
pytest -q
```

Tests marked `gpu` are skipped automatically on machines without a usable NVIDIA GPU.

## Rust port (work in progress)

The app is being ported to Rust in stages; the Python version above stays the one you ship.
See `docs/instruction/rust-port.md` for the stage-by-stage status.

```bash
cd rust
cargo test                 # core logic + CPU parts of the GUI
cargo test --features sr   # adds the Super Resolution (ONNX) test
cargo run -p pbe-gui       # the Rust GUI (egui + wgpu)
```

The Rust GUI reads the **same** `auth.json`, `state.json` and `presets` folder as the Python
version, so don't run both at once. Text layers, the collage dialog, `.pbep` project files and
video playback with sound are not ported yet.

## Development: live reload (no build)

```bat
dev.bat          & rem Windows
```

```bash
./dev.sh         # macOS
```

Runs the app from source (`venv`). Save a `.py` file in `core\`, `ui\` or `main.py` →
the app restarts by itself in ~2 s, on the same page and window position; save
`ui\theme.qss` → the new style is applied instantly. The title bar shows `[DEV — reloaded:
<files>]` and the console logs what changed. If the app crashes (syntax error…), fix the
file and save: it starts again. Close the window or press Ctrl+C to stop.

## Build (production) — one command per OS

PyInstaller does not cross-compile: build the Windows app on Windows and the Mac app on a Mac.

| Target | Run on | Command | Result |
|---|---|---|---|
| Windows 10 / 11 (64-bit) | Windows | `build_windows.bat` | `dist\PhotoBatchEditor\PhotoBatchEditor.exe` (signed) + installer |
| macOS 11+ (MacBook) | Mac | `./build_mac.sh` | `dist/PhotoBatchEditor.app` + `.dmg` |

Add `--skip-tests` to either command to skip pytest. One Windows build runs on both Windows 10
and Windows 11.

### Windows 10 / 11

```bat
build_windows.bat                 & rem tests + .exe + installer
build_windows.bat --skip-tests    & rem faster, no pytest
```

Uses `venv\` (created with `requirements-dev.txt` if missing), runs the tests, copies
`ffmpeg.exe` into `ffmpeg\` (`tools\fetch_ffmpeg.py`, for the Video editor), builds
`dist\PhotoBatchEditor\PhotoBatchEditor.exe` and, if Inno Setup 6 is installed, the installer.
The .exe is built with PyInstaller (`--onedir`, see
`PhotoBatchEditor.spec`). Copy the whole `dist\PhotoBatchEditor` folder to the target
machine; it needs neither Python nor the CUDA Toolkit, only the NVIDIA driver for GPU mode.
The folder is about 1.5–3 GB because of the bundled CUDA/cuDNN libraries.

**Version.** `version.json` holds the release version (`"version": "1.0.0"`, change it by hand)
and a build number that `tools/bump_build.py` increases on every `build_windows.bat` /
`build_mac.sh` run. The .exe shows it in *Properties › Details* (`1.0.0.12`), the installer is
named `PhotoBatchEditor-Setup-1.0.0.12.exe`, the Mac app/dmg use it too.

**Windows Defender / SmartScreen warnings.** An unsigned PyInstaller .exe is often flagged
(false positive, e.g. `Trojan:Win32/Wacatac`) because the prebuilt PyInstaller bootloader is
the same in every PyInstaller app, malware included. `build_windows.bat` therefore compiles
the bootloader from source (`tools\build_bootloader.bat`, once per PyInstaller version). That
needs the free Microsoft C++ Build Tools, installed once:

```bat
winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

Without them the build still works with the prebuilt bootloader (and prints a warning).

**Code signing (self-signed, free).** Every Windows build signs `PhotoBatchEditor.exe` and the
installer (`tools\sign_windows.ps1`, called by `build_windows.bat` / `build_installer.bat`;
SHA-256 + timestamp, certificate `CN=Photo Batch Editor` created once in the build PC's
`Cert:\CurrentUser\My`) and copies the public certificate to
`dist\PhotoBatchEditor\PhotoBatchEditor.cer` and `installer_output\PhotoBatchEditor.cer`.

On the other PC install `PhotoBatchEditor.cer` **once**: double-click → **Install
Certificate…** → *Current User* → *Next* → *Place all certificates in the following store* →
*Browse…* → **Trusted Root Certification Authorities** → *Next* → *Finish* (confirm). From then
on the Setup and the app start with no “Windows protected your PC” / “unknown publisher”
warning. Without the .cer the usual fallback stays: *More info › Run anyway*, or right-click the
file › *Properties* › *Unblock*.

A self-signed certificate is not a paid CA certificate: it proves the file was not modified but
builds no reputation. If a build is still flagged or warned, report it as a false positive
(free) at <https://www.microsoft.com/wdsi/filesubmission> — once Microsoft analyses the file, no
PC warns about it whatever the certificate.

### macOS (MacBook)

```bash
chmod +x build_mac.sh dev.sh lint.sh   # once, if the files lost their "executable" flag
./build_mac.sh                          # tests + .app + .dmg
./build_mac.sh --skip-tests
```

Creates `venv/` with Python 3.12/3.11 if missing (`PYTHON=/path/to/python3 ./build_mac.sh` to
choose), runs the tests, copies the Mac `ffmpeg` into `ffmpeg/`, builds
`dist/PhotoBatchEditor.app` (signed ad-hoc) and `dist/PhotoBatchEditor-<version>-<arch>.dmg` — open
it and drag *Photo Batch Editor* into *Applications*.

* The app is built for the CPU of the Mac that builds it: build on an M1–M4 Mac for Apple
  Silicon (`arm64`), on an Intel Mac for Intel (`x86_64`, also runs on Apple Silicon via Rosetta 2).
* The app is not signed with an Apple Developer ID, so macOS blocks the first start: right-click
  the app › **Open** › **Open**, or run
  `xattr -dr com.apple.quarantine /Applications/PhotoBatchEditor.app`.
* Keyboard shortcuts written `Ctrl+…` in the app are `⌘ Cmd+…` on the Mac.

### Windows installer

```bat
build_installer.bat
```

Packs `dist\PhotoBatchEditor` into `installer_output\PhotoBatchEditor-Setup-<version>.exe`
(Inno Setup 6, `installer.iss`; install it with `winget install JRSoftware.InnoSetup`).
The installer (≈ 1.5 GB) installs to Program Files (or per-user without admin rights),
adds Start Menu / optional desktop shortcuts, a “GPU check” shortcut (runs `--selftest`
and writes `Documents\PhotoBatchEditor-selftest.json`) and an uninstaller. User presets and
settings are kept on uninstall.

**What to give to the user** — the whole `installer_output\` folder (or a zip of it):

| File | The user does |
|---|---|
| `PhotoBatchEditor.cer` | double-click → *Install Certificate…* → *Trusted Root Certification Authorities* (**once per PC**, see above) |
| `PhotoBatchEditor-Setup-<version>.exe` | double-click → install (already signed) → shortcut on the desktop → done, no other file, no Python, no `.bat` |

Nothing else is needed: the installer copies every DLL / model / ffmpeg itself, and after that a
single double-click on the desktop shortcut starts the app.

## Code style: "ESLint for Python" (Ruff + Pyright)

ESLint only understands JavaScript / TypeScript; this Python project uses the equivalents
(rules in `pyproject.toml`):

* **Ruff** — formatter + linter (like ESLint + Prettier). Yellow warnings while you type: trailing or
  missing whitespace, blank lines, unused / unsorted imports, double quotes, common bugs. Red for
  syntax errors and undefined names. Code uses **single quotes** (`'text'`); `"..."` only when the
  text contains a `'`, and docstrings keep `"""` (PEP 257). Lines up to 110 columns.
* **Pyright** (the engine of VS Code's Pylance) — red errors for wrong code: unknown attribute or
  method, wrong number of arguments, missing import, unbound variable, wrong return type.

**VS Code:** install the recommended extensions (*Ruff*, *Python*, *Pylance*). Problems appear while
typing (tab *Problems*: Ctrl+Shift+M); on save (Ctrl+S / ⌘S) the file is formatted, imports are
sorted and safe problems are fixed. Unused imports are only reported, never deleted on save.

**Command line** (also run by `build_windows.bat` / `build_mac.sh` before the tests):

```bat
lint.bat              & rem Windows: format + auto-fix + type check
lint.bat --check      & rem only check, exit code 1 if something is wrong
```

```bash
./lint.sh             # macOS
./lint.sh --check
```

## Where things are stored

On macOS `%APPDATA%\PhotoBatchEditor` is `~/Library/Application Support/PhotoBatchEditor` and
`%LOCALAPPDATA%\PhotoBatchEditor` is `~/Library/Caches/PhotoBatchEditor`.

* Saved filter settings (user presets): `%APPDATA%\PhotoBatchEditor\presets\*.json`
* Last settings, folder, device and the Settings page selection / sample photo:
  `%APPDATA%\PhotoBatchEditor\state.json`
* Sign-in (user name, salted PBKDF2 password hash, “keep me signed in”):
  `%APPDATA%\PhotoBatchEditor\auth.json` — delete it to reset the account to `admin` / `admin`.
* CuPy kernel cache: `%LOCALAPPDATA%\PhotoBatchEditor\cupy_cache`
* Each run writes `process_log.txt` into the output folder.
* Photo / video editor exports: the `editor` folder next to the photo or video (changeable when
  exporting). Photo projects: `.pbep` files wherever you save them (default: the `editor` folder).

## Super Resolution model

`models/realesr-general-x4v3.onnx` was converted from the official Real-ESRGAN weights with
`tools/export_onnx.py` (developer machine only, needs `torch`; see
`tools/requirements-export.txt`). Real-ESRGAN code and weights are BSD-3-Clause, see
`models/LICENSE-Real-ESRGAN.txt`.

## Project layout

```
main.py              entry point (calls multiprocessing.freeze_support())
core/                image processing, no Qt
  settings.py        AdjustmentSettings, limits, JSON
  backend.py         CPU (NumPy/OpenCV) and GPU (CuPy) backends, GPU detection
  adjustments.py     the 15 adjustments (reference + fused fast path)
  enhance.py         Super Resolution (ONNX Runtime, tiled)
  image_size.py      Image Size / resample
  pipeline.py        apply_adjustments(), process_image()
  io_utils.py        reading (Unicode paths, EXIF orientation), JPEG writing
  scanner.py         folder scan, output naming
  batch.py           CPU process pool / 3-stage GPU pipeline, report, CLI
  presets.py         presets (saved filter settings)
  auth.py            local sign-in (auth.json)
ui/                  PySide6 user interface (theme.qss, icons.py, login_view.py,
                     sidebar.py, settings_view.py, prepare/run/results views)
tests/               pytest suite (synthetic images)
  photo/             photo editor helpers: undo history, zoom / crop geometry, blur / pixelate,
                     photo adjustments, JPEG / PNG export, collage layouts
  video/             video editor: project + timeline edits, FFmpeg probe / run, export command
ui/photo/            photo editor page (document + layers, renderer, canvas, tools, panels,
                     .pbep project files, collage dialog)
ui/video/            video editor page (preview player, timeline, panels, text overlays)
rust/                Rust port (see docs/instruction/rust-port.md)
  core/              pbe-core: the Rust port of core/ (incl. photo/ and video/)
  gui/               pbe-gui: the Rust GUI (egui + wgpu), binary photo-batch-editor
tools/export_onnx.py ONNX export of the Super Resolution model
tools/fetch_ffmpeg.py copies ffmpeg(.exe) into ffmpeg/ for the build
tools/make_icns.py   assets/app.ico -> assets/app.icns (macOS icon)
core/system.py       Windows / macOS differences (default font)
build_windows.bat    Windows 10/11 build      build_mac.sh   macOS build (.app + .dmg)
dev.bat / dev.sh     live reload              lint.bat / lint.sh   Ruff format + lint
pyproject.toml       Ruff rules
```
