# -*- mode: python ; coding: utf-8 -*-
# PyInstaller spec for Photo Batch Editor (--onedir, windowed), Windows 10/11 and macOS.
#   Windows: build_windows.bat -> dist/PhotoBatchEditor/PhotoBatchEditor.exe
#   macOS:   ./build_mac.sh    -> dist/PhotoBatchEditor.app
# PyInstaller does not cross-compile: each OS builds its own app.
#
# GPU support without the CUDA Toolkit (spec section 10.6, Windows only): the CUDA 12 runtime
# DLLs come from the nvidia-*-cu12 pip packages (requirements-cuda.txt). They are bundled under
# _internal/nvidia/<component>/bin, the same layout as in site-packages, and registered at
# startup by core.paths.add_cuda_dll_dirs(). CUDA headers (nvidia/*/include) are bundled for
# CuPy's runtime kernel compilation (NVRTC). A Mac has no NVIDIA GPU: CPU only.
import json
import os
import sys

from PyInstaller.utils.hooks import (collect_data_files, collect_dynamic_libs, collect_submodules,
                                     copy_metadata)

IS_MAC = sys.platform == "darwin"
# version.json: "version" set by hand, "build" +1 per build (tools/bump_build.py, which also
# writes the Windows version resource build/version_info.txt shown in Properties > Details)
with open(os.path.join(SPECPATH, "version.json"), encoding="utf-8") as f:  # noqa: F821
    _ver = json.load(f)
VERSION = _ver["version"]
BUILD = str(_ver["build"])
VERSION_INFO = os.path.join(SPECPATH, "build", "version_info.txt")  # noqa: F821
if not IS_MAC and not os.path.isfile(VERSION_INFO):
    print(f"[spec] {VERSION_INFO} missing (run tools/bump_build.py): the .exe has no version info")

datas = [
    ("ui/theme.qss", "ui"),
    ("presets_builtin", "presets_builtin"),
    ("models", "models"),
    ("version.json", "."),  # read by core.version at runtime
]
binaries = []
hiddenimports = []

# Video editor: ffmpeg copied into ffmpeg/ by tools/fetch_ffmpeg.py (the build script runs it);
# found at runtime by core.video.ffmpeg.find_ffmpeg() as <app_root>/ffmpeg/ffmpeg(.exe)
FFMPEG = "ffmpeg/ffmpeg" if IS_MAC else "ffmpeg/ffmpeg.exe"
if os.path.isfile(FFMPEG):
    if IS_MAC:  # a Mach-O executable must go to Contents/Frameworks (signed with the app)
        binaries += [(FFMPEG, "ffmpeg")]
    else:
        datas += [(FFMPEG, "ffmpeg")]
    datas += [("ffmpeg/README.txt", "ffmpeg")]
else:
    print(f"[spec] {FFMPEG} missing (run tools/fetch_ffmpeg.py): the Video editor cannot export")

if not IS_MAC:
    # CuPy (+ its backends and the library locator it uses)
    for pkg in ("cupy", "cupyx", "cupy_backends", "cuda_pathfinder", "fastrlock"):
        try:
            hiddenimports += collect_submodules(pkg, filter=lambda name: ".tests" not in name)
            datas += collect_data_files(pkg)
            binaries += collect_dynamic_libs(pkg)
        except Exception as exc:  # package missing on this build machine
            print(f"[spec] skipping {pkg}: {exc}")
    for dist in ("cupy-cuda12x", "onnxruntime-gpu", "cuda-pathfinder"):
        try:
            datas += copy_metadata(dist)
        except Exception as exc:
            print(f"[spec] no metadata for {dist}: {exc}")

# ONNX Runtime (CUDA + CPU providers on Windows, CPU on macOS)
hiddenimports += collect_submodules("onnxruntime", filter=lambda name: "tools" not in name
                                    and "transformers" not in name and "quantization" not in name)
binaries += collect_dynamic_libs("onnxruntime")

if not IS_MAC:
    # CUDA 12 runtime libraries (cuBLAS, cuDNN 9, cuFFT, cuRAND, NVRTC, …) and headers
    UNUSED_CUDA = ("cusolver", "cusparse")  # linear algebra: not used by the app (saves ~1 GB)
    try:
        binaries += [b for b in collect_dynamic_libs("nvidia")
                     if not any(f"nvidia{sep}{u}" in b[1] for u in UNUSED_CUDA for sep in "/\\")]
        datas += collect_data_files("nvidia", includes=["**/include/**"])
    except Exception as exc:
        print(f"[spec] nvidia CUDA runtime packages not found ({exc}); the .exe will run on CPU only")

a = Analysis(
    ["main.py"],
    pathex=[],
    binaries=binaries,
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=["torch", "tkinter", "matplotlib", "PyQt5", "PyQt6", "IPython", "pytest", "scipy",
              "pandas", "onnx", "ruff",
              "imageio_ffmpeg"],  # its ffmpeg is bundled once, in ffmpeg/ (see above)
    noarchive=False,
    optimize=0,
)
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="PhotoBatchEditor",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=False,
    console=False,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,  # macOS: the architecture of the Python that builds (arm64 or x86_64)
    codesign_identity=None,  # macOS: ad-hoc signature
    entitlements_file=None,
    icon="assets/app.icns" if IS_MAC else "assets/app.ico",
    version=None if IS_MAC or not os.path.isfile(VERSION_INFO) else VERSION_INFO,
)
coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    strip=False,
    upx=False,
    upx_exclude=[],
    name="PhotoBatchEditor",
)

if IS_MAC:
    app = BUNDLE(
        coll,
        name="PhotoBatchEditor.app",
        icon="assets/app.icns",
        bundle_identifier="com.photobatcheditor.app",
        version=VERSION,
        info_plist={
            "CFBundleDisplayName": "Photo Batch Editor",
            "CFBundleName": "Photo Batch Editor",
            "CFBundleShortVersionString": VERSION,
            "CFBundleVersion": BUILD,
            "LSMinimumSystemVersion": "11.0",
            "NSHighResolutionCapable": True,
            "NSRequiresAquaSystemAppearance": False,
            "LSApplicationCategoryType": "public.app-category.photography",
        },
    )
