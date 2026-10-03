"""Folder scanning and naming (spec sections 4, 7.2, 7.4)."""

from __future__ import annotations

from pathlib import Path

import pytest

from core.scanner import OutputFolderError, default_output_dir, next_free_output_dir, plan_jobs, scan_folder


def touch(p: Path) -> Path:
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_bytes(b'x')
    return p


def test_extension_filter_case_insensitive(tmp_path: Path):
    for name in [
        'a.JPG',
        'b.jpeg',
        'c.PNG',
        'd.tif',
        'e.TIFF',
        'f.webp',
        'g.bmp',
        'h.txt',
        'i.cr2',
        'j.gif',
        '.hidden.jpg',
        'Thumbs.db',
        'desktop.ini',
    ]:
        touch(tmp_path / name)
    names = [p.name for p in scan_folder(tmp_path)]
    assert names == ['a.JPG', 'b.jpeg', 'c.PNG', 'd.tif', 'e.TIFF', 'f.webp', 'g.bmp']


def test_subfolders_and_order(tmp_path: Path):
    touch(tmp_path / 'b.jpg')
    touch(tmp_path / 'A.jpg')
    touch(tmp_path / 'outdoor' / 'x.png')
    touch(tmp_path / '.cache' / 'y.png')
    files = scan_folder(tmp_path)
    rel = [p.relative_to(tmp_path).as_posix() for p in files]
    assert rel == ['A.jpg', 'b.jpg', 'outdoor/x.png']
    out = tmp_path.parent / 'out'
    jobs = plan_jobs(tmp_path, files, out)
    assert jobs[2].output_path == out / 'outdoor' / 'x.jpg'
    assert jobs[2].rel_dir == 'outdoor' and jobs[0].rel_dir == ''


def test_name_collision(tmp_path: Path):
    files = [touch(tmp_path / 'a.png'), touch(tmp_path / 'a.jpg'), touch(tmp_path / 'a.tif')]
    jobs = plan_jobs(tmp_path, scan_folder(tmp_path), tmp_path.parent / 'o')
    by_input = {j.input_path.name: j for j in jobs}
    assert by_input['a.jpg'].output_path.name == 'a.jpg' and not by_input['a.jpg'].renamed
    assert by_input['a.png'].output_path.name == 'a_1.jpg' and by_input['a.png'].renamed
    assert by_input['a.tif'].output_path.name == 'a_2.jpg'
    assert len({j.output_path for j in jobs}) == len(files)


def test_name_keeps_unicode_and_lowercase_ext(tmp_path: Path):
    touch(tmp_path / 'IMG_001.PNG')
    touch(tmp_path / 'ảnh cưới 01.jpeg')
    names = sorted(j.output_path.name for j in plan_jobs(tmp_path, scan_folder(tmp_path), tmp_path / 'o'))
    assert names == ['IMG_001.jpg', 'ảnh cưới 01.jpg']


def test_output_folder_names(tmp_path: Path):
    src = tmp_path / 'clientA'
    src.mkdir()
    assert default_output_dir(src) == tmp_path / 'clientA_update'
    assert next_free_output_dir(src) == tmp_path / 'clientA_update'
    (tmp_path / 'clientA_update').mkdir()
    assert next_free_output_dir(src) == tmp_path / 'clientA_update_2'
    (tmp_path / 'clientA_update_2').mkdir()
    assert next_free_output_dir(src) == tmp_path / 'clientA_update_3'


def test_drive_root_rejected():
    root = Path(Path.cwd().anchor)
    with pytest.raises(OutputFolderError):
        default_output_dir(root)
