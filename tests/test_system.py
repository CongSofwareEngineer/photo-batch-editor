"""Windows / macOS differences: requirements, Finder, cache folder, fonts, build files."""

from __future__ import annotations

import sys
from pathlib import Path

import pytest
from packaging.requirements import Requirement

from core import io_utils, paths, system

ROOT = Path(__file__).resolve().parent.parent


def _packages(req_file: str, platform: str) -> set[str]:
    """Package names pip installs from ``req_file`` on ``sys_platform == platform``."""
    out: set[str] = set()
    for raw in (ROOT / req_file).read_text(encoding='utf-8').splitlines():
        line = raw.split('#', 1)[0].strip()
        if not line:
            continue
        if line.startswith('-r '):
            out |= _packages(line[3:].strip(), platform)
            continue
        req = Requirement(line)
        if req.marker is None or req.marker.evaluate({'sys_platform': platform}):
            out.add(req.name.lower())
    return out


def test_requirements_mac_has_no_cuda_packages():
    mac = _packages('requirements-dev.txt', 'darwin')
    assert 'onnxruntime' in mac and 'pyside6' in mac and 'imageio-ffmpeg' in mac
    assert not {'cupy-cuda12x', 'onnxruntime-gpu'} & mac


def test_requirements_windows_keeps_gpu_packages():
    win = _packages('requirements-dev.txt', 'win32')
    assert {'cupy-cuda12x', 'onnxruntime-gpu', 'ruff', 'pyinstaller'} <= win
    assert 'onnxruntime' not in win  # never next to onnxruntime-gpu


class _Popen:
    calls: list = []

    def __init__(self, args, *a, **kw):  # noqa: ANN001
        _Popen.calls.append(args)


@pytest.fixture
def fake_popen(monkeypatch):
    _Popen.calls = []
    monkeypatch.setattr(io_utils.subprocess, 'Popen', _Popen)
    return _Popen.calls


def test_mac_reveals_in_finder(monkeypatch, fake_popen, tmp_path):
    monkeypatch.setattr(sys, 'platform', 'darwin')
    f = tmp_path / 'a.jpg'
    io_utils.open_in_file_manager(f, select=True)
    io_utils.open_in_file_manager(tmp_path)
    io_utils.open_with_default_app(f)
    assert fake_popen == [['open', '-R', str(f)], ['open', str(tmp_path)], ['open', str(f)]]


def test_linux_uses_xdg_open(monkeypatch, fake_popen, tmp_path):
    monkeypatch.setattr(sys, 'platform', 'linux')
    io_utils.open_in_file_manager(tmp_path / 'a.jpg', select=True)
    assert fake_popen == [['xdg-open', str(tmp_path)]]


def test_mac_cache_dir(monkeypatch, tmp_path):
    monkeypatch.delenv('LOCALAPPDATA', raising=False)
    monkeypatch.setattr(sys, 'platform', 'darwin')
    monkeypatch.setattr(paths.Path, 'home', classmethod(lambda cls: tmp_path))
    assert paths.local_appdata_dir() == tmp_path / 'Library' / 'Caches' / 'PhotoBatchEditor'


def test_default_font_matches_os():
    expected = {'win32': 'Segoe UI', 'darwin': 'Helvetica Neue'}.get(sys.platform, 'DejaVu Sans')
    assert expected == system.DEFAULT_FONT_FAMILY
    from core.video.project import TextOverlay

    assert TextOverlay().font_family == expected


def test_build_files_for_both_os():
    spec = (ROOT / 'PhotoBatchEditor.spec').read_text(encoding='utf-8')
    assert 'BUNDLE(' in spec and 'assets/app.icns' in spec and 'assets/app.ico' in spec
    assert (ROOT / 'assets' / 'app.icns').stat().st_size > 1000
    assert (ROOT / 'build_windows.bat').is_file()
    for sh in ('build_mac.sh', 'dev.sh', 'lint.sh'):
        data = (ROOT / sh).read_bytes()
        assert data.startswith(b'#!/usr/bin/env bash') and b'\r\n' not in data, sh  # bash needs LF
    bat = (ROOT / 'build_windows.bat').read_bytes()
    assert b'tools/fetch_ffmpeg.py' in bat and b'\x0c' not in bat


def test_windows_build_signs_exe_and_installer():
    ps1 = (ROOT / 'tools' / 'sign_windows.ps1').read_text(encoding='utf-8')
    assert 'New-SelfSignedCertificate' in ps1 and 'Set-AuthenticodeSignature' in ps1
    assert 'CN=Photo Batch Editor' in ps1 and 'Get-AuthenticodeSignature' in ps1
    win = (ROOT / 'build_windows.bat').read_text(encoding='utf-8')
    assert 'tools\\sign_windows.ps1' in win and '-CopyCertificateTo "dist\\PhotoBatchEditor"' in win
    ins = (ROOT / 'build_installer.bat').read_bytes()
    assert ins.count(b'-File "tools\\sign_windows.ps1"') == 2 and b'\x0c' not in ins
    assert b'-CopyCertificateTo "installer_output"' in ins
