# Photo Batch Editor

Desktop app (Windows 10/11 64-bit and macOS 11+ — Apple Silicon or Intel MacBook) written in
**Rust** (egui + wgpu). It applies the same Camera Raw–style look to a whole folder of photos:
choose a folder → pick a preset or set the adjustments → **RUN** → the edited photos are written
as JPEG quality 100 (4:4:4) to the sibling folder `<folder>_update`, with the same file names and
sub-folders. Originals are never modified.

* 15 adjustments named like Camera Raw (Temperature, Tint, Exposure, Brightness, Contrast,
  Highlights, Shadows, Whites, Blacks, Clarity, Vibrance, Saturation, Sharpening › Amount,
  Noise Reduction, Post-Crop Vignetting › Amount) + **Super Resolution** (Real-ESRGAN, 2x/4x)
  + **Image Size** (Photoshop resample methods).
* Live preview, built-in and user presets, results list with a large before/after viewer.
* **Sign-in** on first start (default account `admin` / `admin`, change it in
  Settings › Account). With “Keep me signed in” the app opens directly next time;
  “Sign out” in the sidebar asks for the password again.
* **Settings page** (sidebar): create as many saved filter settings as you like (blank, from the
  Editor's current adjustments, from a built-in template, or imported from a `.json` file), edit
  them with a live preview on a sample photo, rename, duplicate, export, delete. Every change is
  saved automatically; pick a setting in the Editor's “Setting” box to apply it.
* **Photo editor** (sidebar) — one photo, Photoshop-style, with layers: inserted images (PNG
  transparency, move / resize / rotate), blur or pixelate areas, crop (free, original, 1:1, 4:3,
  16:9, 9:16), rotate 90° / straighten / flip, brightness / contrast / saturation / temperature /
  sharpness (same algorithms as the batch). Ctrl + wheel zooms at the cursor, Space + drag pans,
  Ctrl+0 fit, Ctrl+Z / Ctrl+Y (undo/redo). Export JPEG (quality) or PNG.
* **Video editor** (sidebar) — preview, timeline, split / reorder / delete clips, In / Out marks,
  mute, music (start offset, own volume, fade out). Export MP4 or MOV, original / 1080p / 720p, in
  the background with a progress bar and Cancel, to `<video folder>\editor` (the original is never
  overwritten). Uses FFmpeg, bundled in `ffmpeg/`.

* **English / Tiếng Việt**: choose the language in Settings › General (or on the sign-in page). The
  window is rebuilt immediately — folder, adjustments and settings are kept. Texts live in
  `rust/core/src/i18n_vi.rs` / `rust/core/src/i18n_vi_data.json` (English text = key).

## Requirements

* Windows 10/11 64-bit, or macOS 11 Big Sur or newer (Apple Silicon M1–M4 or Intel). Linux works
  for development/tests.
* **Rust stable** toolchain (via [rustup](https://rustup.rs)). On macOS the system `clang` linker
  is used.
* The current Rust port runs on the **CPU**. The GPU (CUDA/CuPy) branch of the Python version was
  not ported. Super Resolution is available behind the Cargo feature `sr` (pulls ONNX Runtime).
* FFmpeg for the Video editor: put the binary in `ffmpeg/` (or set `PBE_FFMPEG`, or have `ffmpeg`
  on `PATH`).

## Run

```bash
cd rust
cargo run -p pbe-gui       # the GUI (egui + wgpu)
```

## Tests

```bash
cd rust
cargo test                 # core logic + CPU parts of the GUI
cargo test --features sr   # adds the Super Resolution (ONNX) test
cargo test --test video_export   # real FFmpeg export (skipped when FFmpeg is missing)
```

The core test suite is a port of the old Python `tests/` suite (same inputs → same results within
the tolerance the Python tests allowed); reference fixtures live in `rust/core/tests/fixtures/`.

## Build (production) — one command per OS

| Target | Run on | Command | Result |
|---|---|---|---|
| Windows 10 / 11 (64-bit) | Windows | `build_rust_windows.bat` | `rust\target\release\photo-batch-editor.exe` |
| macOS 11+ (MacBook) | Mac | `./build_rust_mac.sh` | `rust/target/release/bundle/osx/Photo Batch Editor.app` |

The macOS script builds the release binary and bundles it with `cargo-bundle`, copying `assets/`,
`models/`, `ffmpeg/` and the README into the app. The Windows script builds the release `.exe`.
Rust does not cross-compile: build the Windows app on Windows and the Mac app on a Mac.

The app version comes from `version.json` at the repository root (`{"version": "1.0.0", "build": N}`,
change `version` by hand when releasing).

## Where things are stored

On macOS `%APPDATA%\PhotoBatchEditor` is `~/Library/Application Support/PhotoBatchEditor` and
`%LOCALAPPDATA%\PhotoBatchEditor` is `~/Library/Caches/PhotoBatchEditor`.

* Saved filter settings (user presets): `%APPDATA%\PhotoBatchEditor\presets\*.json`
* Last settings, folder, device and the Settings page selection / sample photo:
  `%APPDATA%\PhotoBatchEditor\state.json`
* Sign-in (user name, salted PBKDF2 password hash, “keep me signed in”):
  `%APPDATA%\PhotoBatchEditor\auth.json` — delete it to reset the account to `admin` / `admin`.
* Each run writes `process_log.txt` into the output folder.
* Photo / video editor exports go to the `editor` folder next to the photo or video.

The GUI reads the **same** `auth.json`, `state.json` and `presets` folder as the old Python version
did, so existing user data keeps working.

## Super Resolution model

`models/realesr-general-x4v3.onnx` is the official Real-ESRGAN weights converted to ONNX. The ONNX
export tool was removed together with the Python port; the model file is kept in the repository.
Real-ESRGAN code and weights are BSD-3-Clause, see `models/LICENSE-Real-ESRGAN.txt`.

## Not yet ported

The Rust GUI does not yet have: text layers in the photo editor, drawing text onto video, the
collage dialog (the layout logic exists in `pbe-core`), `.pbep` project files, video playback with
sound (preview is a still frame extracted with FFmpeg), the full-screen image viewer, tray
notifications and list thumbnails. See `docs/instruction/rust-port.md` for the full status table.

## Project layout

```
rust/                    Cargo workspace
  core/                  pbe-core — all logic (no UI)
    src/                 version, system, paths, settings, scanner, auth, i18n,
                         presets, image_size, resize, backend, adjustments, pipeline,
                         io_utils, enhance (feature sr), batch
    src/photo/           photo document: effects, geometry, history, collage, image_io
    src/video/           video: ffmpeg, project, export
    tests/               port of the old Python suite + reference fixtures
  gui/                   pbe-gui — egui + wgpu GUI, binary photo-batch-editor
    src/                 app shell, sidebar, login, batch, adjust, preview,
                         photo editor, video editor, settings, theme
    tests/               CPU render tests (framebuffer, compositing, crop/rotate)
models/                  Super Resolution ONNX model
presets_builtin/         built-in filter settings
ffmpeg/                  FFmpeg binary for the Video editor (not committed)
assets/                  app icons
version.json             app version + build number
docs/                    per-logic documentation (see docs/log.md)
build_rust_windows.bat   Windows build        build_rust_mac.sh   macOS build (.app)
```

## Code style

Rust code follows the standard `rustfmt` / `clippy` conventions:

```bash
cd rust
cargo fmt                 # format
cargo clippy --all-targets
```
