"""Presets (spec section 8.9)."""

from __future__ import annotations

import json
from pathlib import Path

from core.presets import (
    delete_user_preset,
    describe_settings,
    list_builtin_presets,
    list_user_presets,
    load_preset,
    rename_user_preset,
    save_user_preset,
    unique_name,
)
from core.settings import AdjustmentSettings


def write(tmp_path: Path, settings: dict) -> Path:
    p = tmp_path / 'p.json'
    p.write_text(json.dumps({'version': 2, 'name': 'Test', 'settings': settings}), encoding='utf-8')
    return p


def test_builtin_presets_load():
    presets = {p.name: p for p in list_builtin_presets()}
    assert list(presets) == [
        'Default',
        'Bright & Clean',
        'Warm',
        'High Contrast',
        'Black & White',
        'Web Export',
    ]
    assert presets['Default'].settings.is_default()
    b = presets['Bright & Clean'].settings
    assert (b.exposure, b.shadows, b.highlights, b.vibrance) == (0.3, 30, -20, 15)
    w = presets['Warm'].settings
    assert (w.temperature, w.tint, w.vibrance) == (25, 5, 10)
    hc = presets['High Contrast'].settings
    assert (hc.contrast, hc.clarity, hc.blacks, hc.whites) == (30, 25, -15, 10)
    bw = presets['Black & White'].settings
    assert (bw.saturation, bw.contrast, bw.clarity) == (-100, 20, 15)
    web = presets['Web Export'].settings
    assert web.image_size.mode == 'long_edge' and web.image_size.value == 2048
    assert web.image_size.resample == 'bicubic_sharper' and web.sharpening_amount == 25
    assert all(p.builtin for p in presets.values())


def test_missing_keys_default(tmp_path: Path):
    s = load_preset(write(tmp_path, {'exposure': 1.5})).settings
    assert s.exposure == 1.5 and s.contrast == 0 and s.super_resolution == 'off'
    assert s.image_size.mode == 'off'


def test_unknown_keys_ignored(tmp_path: Path):
    s = load_preset(write(tmp_path, {'dehaze': 40, 'contrast': 10})).settings
    assert s.contrast == 10 and not hasattr(s, 'dehaze')


def test_out_of_range_clamped(tmp_path: Path):
    s = load_preset(
        write(
            tmp_path,
            {
                'exposure': 9,
                'contrast': -500,
                'sharpening_amount': -3,
                'image_size': {'mode': 'percent', 'value': 1000},
            },
        )
    ).settings
    assert s.exposure == 5 and s.contrast == -100 and s.sharpening_amount == 0
    assert s.image_size.value == 400


def test_invalid_enums_default(tmp_path: Path):
    s = load_preset(
        write(
            tmp_path,
            {
                'super_resolution': '8x',
                'contrast': 'abc',
                'image_size': {'mode': 'huge', 'resample': 'magic', 'value': 50},
            },
        )
    ).settings
    assert s.super_resolution == 'off' and s.contrast == 0
    assert s.image_size.mode == 'off' and s.image_size.resample == 'automatic'
    s = load_preset(write(tmp_path, {'image_size': {'mode': 'width', 'resample': 'lanczos'}})).settings
    assert s.image_size.mode == 'width' and s.image_size.resample == 'automatic'


def test_user_preset_roundtrip(tmp_path: Path):
    s = AdjustmentSettings(exposure=0.35, super_resolution='2x')
    s.image_size.mode, s.image_size.value = 'width', 1200
    path = save_user_preset(tmp_path, 'My: Look', s)
    assert path.exists() and ':' not in path.name
    [p] = list_user_presets(tmp_path)
    assert p.name == 'My: Look' and p.settings.to_dict() == s.to_dict() and not p.builtin
    path2 = save_user_preset(tmp_path, 'My: Look', s)
    assert path2 != path
    delete_user_preset(p)
    assert not path.exists()


def test_rename_user_preset(tmp_path: Path):
    s = AdjustmentSettings(contrast=12)
    path = save_user_preset(tmp_path, 'Old', s)
    [p] = list_user_presets(tmp_path)
    q = rename_user_preset(p, '  New name ')
    assert q.name == 'New name' and q.path == path
    [r] = list_user_presets(tmp_path)
    assert r.name == 'New name' and r.settings.contrast == 12
    builtin = list_builtin_presets()[0]
    for bad in ((builtin, 'x'), (r, '   ')):
        try:
            rename_user_preset(*bad)
        except ValueError:
            pass
        else:
            raise AssertionError('expected ValueError')


def test_unique_name():
    assert unique_name('Warm', ['Default']) == 'Warm'
    assert unique_name('warm', ['Warm']) == 'warm (2)'
    assert unique_name('Warm', ['Warm', 'Warm (2)']) == 'Warm (3)'
    assert unique_name('Warm (2)', ['Warm', 'Warm (2)']) == 'Warm (3)'
    assert unique_name('  ', []) == 'Setting'


def test_describe_settings():
    assert describe_settings(AdjustmentSettings()) == []
    s = AdjustmentSettings(exposure=0.3, contrast=-15, sharpening_amount=40, super_resolution='2x')
    s.image_size.mode, s.image_size.value = 'long_edge', 2048
    assert describe_settings(s) == [
        'Exposure +0.30',
        'Contrast -15',
        'Sharpening 40',
        'Super Res 2x',
        'Resize Long Edge 2048 px',
    ]
