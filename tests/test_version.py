"""App version (version.json) and the per-build bump (tools/bump_build.py)."""

import json
from pathlib import Path

from core.version import AppVersion, read_version, write_version

ROOT = Path(__file__).resolve().parent.parent


def test_repo_version_file_is_valid():
    ver = read_version(ROOT / 'version.json')
    assert ver.version.count('.') == 2 and ver.build >= 0
    assert ver.full.count('.') == 3


def test_numbers_and_full():
    assert AppVersion('1.2.3', 45).numbers == (1, 2, 3, 45)
    assert AppVersion('2.0', 7).full == '2.0.0.7'


def test_missing_or_broken_file(tmp_path):
    assert read_version(tmp_path / 'nope.json') == AppVersion()
    bad = tmp_path / 'v.json'
    bad.write_text('{oops', encoding='utf-8')
    assert read_version(bad) == AppVersion()


def test_write_read_roundtrip(tmp_path):
    f = tmp_path / 'v.json'
    write_version(f, AppVersion('1.4.0', 9))
    assert json.loads(f.read_text(encoding='utf-8')) == {'version': '1.4.0', 'build': 9}
    assert read_version(f) == AppVersion('1.4.0', 9)


def test_version_info_resource(tmp_path):
    from tools.bump_build import write_version_info

    out = tmp_path / 'version_info.txt'
    write_version_info(AppVersion('1.0.0', 12), out)
    text = out.read_text(encoding='utf-8')
    assert 'filevers=(1, 0, 0, 12)' in text and "'1.0.0.12'" in text


def test_build_scripts_bump_version_and_bootloader():
    win = (ROOT / 'build_windows.bat').read_bytes()
    assert b'tools/bump_build.py' in win and b'tools\\build_bootloader.bat' in win
    assert (ROOT / 'tools' / 'build_bootloader.bat').read_bytes().count(b'\r\n') > 10  # CRLF
    assert b'bump_build.py' in (ROOT / 'build_mac.sh').read_bytes()
    assert b'/DAppVersion=' in (ROOT / 'build_installer.bat').read_bytes()
    spec = (ROOT / 'PhotoBatchEditor.spec').read_text(encoding='utf-8')
    assert 'version.json' in spec and 'version=' in spec
