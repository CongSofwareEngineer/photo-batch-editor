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
* **Photo editor** (sidebar) — one photo, Photoshop-style layout: menu bar (File / Edit / Image /
  View) + tool options bar on top, vertical toolbar on the left (Move V, Crop C, Blur B, Insert
  image I, Collage L, Hand H, Zoom Z), panels on the right, status bar at the bottom. Layers:
  inserted images (PNG transparency, move, resize with corner handles — Shift keeps the ratio) and
  blur / pixelate areas; crop (free, 1:1, 4:3, 16:9, 9:16, rule-of-thirds grid), rotate 90° /
  straighten / flip, brightness / contrast / saturation / temperature / sharpness / clarity (same
  algorithms as the batch), **Unsharp Mask** sharpening (Amount / Radius / Threshold, Light /
  Medium / Strong presets). **Background removal** (AI, MODNet): cut the person out of the
  background, check the selection (the part to remove is shown in red) and fix it with an Add /
  Subtract brush before applying, then keep it transparent, use an ID-photo colour (white, blue, light blue, red, grey, or
  any colour) or another photo as the new background; shift / feather the edge and clean the old
  background colour out of hair; insert a PNG (clothes, accessories) on top. **Collage** window: 8 layouts, size, spacing, corner radius,
  background colour, drag / scroll to frame each photo, then open the result in the editor.
  Wheel zooms at the cursor, Space + drag pans, Ctrl+0 fit, Ctrl+1 100 %, Ctrl+Z / Ctrl+Shift+Z,
  Ctrl+J duplicate layer, Ctrl+E export. Export JPEG (quality) or PNG.
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
  Background removal uses the `bgremove` feature of `pbe-core` (ONNX Runtime), always enabled by the
  GUI; the ONNX Runtime binaries are downloaded by the `ort` crate on the first build.
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
cargo test -p pbe-core --features bgremove --test bg_remove_onnx   # background removal (MODNet)
cargo test --test video_export   # real FFmpeg export (skipped when FFmpeg is missing)
```

The core test suite is a port of the old Python `tests/` suite (same inputs → same results within
the tolerance the Python tests allowed); reference fixtures live in `rust/core/tests/fixtures/`.

## Build (production) — one command per OS

| Target | Run on | Command | Result |
|---|---|---|---|
| Windows 10 / 11 (64-bit) | Windows | `build_rust_windows.bat` | `installer_output\PhotoBatchEditor-Setup-X.Y.Z.build.exe` |
| macOS 11+ (MacBook) | Mac | `./build_rust_mac.sh` | `installer_output/PhotoBatchEditor-X.Y.Z.build.dmg` |

**Every build gets a new version:** both scripts add 1 to `build` in `version.json` (the app version is
`X.Y.Z.build`; change `X.Y.Z` by hand when releasing). Commit `version.json` after building.

**Windows** — needs [Inno Setup 6](https://jrsoftware.org/isinfo.php)
(`winget install JRSoftware.InnoSetup`) and `ffmpeg\ffmpeg.exe`. The script builds the release `.exe`
(with icon + version info), signs it, packs it with `models/`, `presets_builtin/`, `ffmpeg/` and
`version.json` into an installer (`installer.iss`: Program Files, Start Menu / Desktop shortcut,
uninstall from Apps & features) and signs the Setup and uninstaller (`tools/sign_windows.ps1`).

* Default: a **self-signed** certificate created on the build PC (the first time Windows asks to trust
  it — click Yes). Send `installer_output\PhotoBatchEditor.cer` and `CaiChungChi.bat` together with the
  Setup; on the other PC run `CaiChungChi.bat` once (it installs the certificate and unblocks the Setup),
  then the Setup runs without the SmartScreen "Run anyway" prompt.
* To avoid the prompt on **every** PC without that step, buy a code-signing certificate and set
  `PBE_SIGN_PFX` + `PBE_SIGN_PFX_PASSWORD` (or `PBE_SIGN_THUMBPRINT` for a USB token) before building.

**macOS** — needs `cargo install cargo-bundle` and a macOS `ffmpeg/ffmpeg`. The script builds the
`.app`, copies `version.json`, `models/`, `presets_builtin/`, `ffmpeg/` into `Contents/Resources`,
signs it and makes a `.dmg`. Without an Apple Developer ID it is signed ad-hoc (other Macs: right-click ›
Open the first time). With one: `export PBE_MAC_SIGN_IDENTITY="Developer ID Application: … (TEAMID)"`
and, to notarize, `xcrun notarytool store-credentials pbe-notary` once +
`export PBE_NOTARY_PROFILE=pbe-notary` — then it opens on any Mac without warnings.

Rust does not cross-compile: build the Windows app on Windows and the Mac app on a Mac.

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

## Models

### Background removal

`models/modnet.onnx` is MODNet (portrait matting, [ZHKKKe/MODNet](https://github.com/ZHKKKe/MODNet)),
ONNX export from [Xenova/modnet](https://huggingface.co/Xenova/modnet), Apache-2.0 — see
`models/LICENSE-MODNet.txt`. It is made for portraits; other subjects may not cut out cleanly.

### Super Resolution

`models/realesr-general-x4v3.onnx` is the official Real-ESRGAN weights converted to ONNX. The ONNX
export tool was removed together with the Python port; the model file is kept in the repository.
Real-ESRGAN code and weights are BSD-3-Clause, see `models/LICENSE-Real-ESRGAN.txt`.

## Not yet ported

The Rust GUI does not yet have: text layers in the photo editor, drawing text onto video, `.pbep` project files, video playback with
sound (preview is a still frame extracted with FFmpeg), the full-screen image viewer, tray
notifications and list thumbnails. See `docs/instruction/rust-port.md` for the full status table.

## Project layout

```
rust/                    Cargo workspace
  core/                  pbe-core — all logic (no UI)
    src/                 version, system, paths, settings, scanner, auth, i18n,
                         presets, image_size, resize, backend, adjustments, pipeline,
                         io_utils, enhance (feature sr), bg_remove (feature bgremove), batch
    src/photo/           photo document: effects, geometry, history, collage, image_io, matting
    src/video/           video: ffmpeg, project, export
    tests/               port of the old Python suite + reference fixtures
  gui/                   pbe-gui — egui + wgpu GUI, binary photo-batch-editor
    src/                 app shell, sidebar, login, batch, adjust, preview,
                         photo editor, video editor, settings, theme
    tests/               CPU render tests (framebuffer, compositing, crop/rotate)
models/                  ONNX models: Super Resolution, background removal (MODNet)
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
