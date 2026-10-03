"""Built-in and user presets (spec section 8.9)."""

from __future__ import annotations

import logging
import re
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path

from core.i18n import tr
from core.paths import resource_path
from core.settings import (
    IMAGE_SIZE_MODES,
    SLIDERS,
    AdjustmentSettings,
    load_settings_json,
    save_settings_json,
)

log = logging.getLogger(__name__)

BUILTIN_ORDER = ('default', 'bright_clean', 'warm', 'high_contrast', 'black_white', 'web_export')


@dataclass
class Preset:
    name: str
    settings: AdjustmentSettings
    path: Path | None
    builtin: bool


def display_name(p: Preset) -> str:
    """Name shown in the UI: built-in templates are translated, user settings never are."""
    return tr(p.name) if p.builtin else p.name


def builtin_dir() -> Path:
    return resource_path('presets_builtin')


def load_preset(path: Path, builtin: bool = False) -> Preset:
    name, settings = load_settings_json(path)
    return Preset(name, settings, Path(path), builtin)


def _load_dir(folder: Path, builtin: bool) -> list[Preset]:
    out: list[Preset] = []
    if not folder.is_dir():
        return out
    for p in folder.glob('*.json'):
        try:
            out.append(load_preset(p, builtin))
        except Exception as exc:  # noqa: BLE001 - a broken preset must not break the app
            log.warning('Skipping preset %s: %s', p, exc)
    return out


def list_builtin_presets() -> list[Preset]:
    presets = _load_dir(builtin_dir(), builtin=True)
    rank = {k: i for i, k in enumerate(BUILTIN_ORDER)}
    presets.sort(key=lambda p: (rank.get(p.path.stem if p.path else '', 99), p.name.casefold()))
    return presets


def list_user_presets(user_dir: Path) -> list[Preset]:
    presets = _load_dir(Path(user_dir), builtin=False)
    presets.sort(key=lambda p: p.name.casefold())
    return presets


def _safe_filename(name: str) -> str:
    cleaned = re.sub(r'[<>:"/\\|?*\x00-\x1f]', '_', name).strip(' .')
    return cleaned[:80] or 'preset'


def save_user_preset(
    user_dir: Path, name: str, settings: AdjustmentSettings, path: Path | None = None
) -> Path:
    """Save a user preset. Without ``path`` a new, unused file name is chosen."""
    user_dir = Path(user_dir)
    if path is None:
        base = _safe_filename(name)
        path = user_dir / f'{base}.json'
        n = 2
        while path.exists():
            path = user_dir / f'{base} ({n}).json'
            n += 1
    save_settings_json(path, settings, name)
    return path


def delete_user_preset(preset: Preset) -> None:
    if preset.builtin or preset.path is None:
        raise ValueError('Built-in presets cannot be deleted')
    preset.path.unlink(missing_ok=True)


def rename_user_preset(preset: Preset, new_name: str) -> Preset:
    """Rename a user preset in place (the file keeps its name, the stored name changes)."""
    new_name = new_name.strip()
    if preset.builtin or preset.path is None:
        raise ValueError('Built-in presets cannot be renamed')
    if not new_name:
        raise ValueError('The name cannot be empty')
    save_settings_json(preset.path, preset.settings, new_name)
    return Preset(new_name, preset.settings, preset.path, False)


def unique_name(name: str, existing: Iterable[str]) -> str:
    """``name``, or ``name (2)``, ``name (3)``… so it differs (case-insensitively) from ``existing``."""
    name = name.strip() or 'Setting'
    taken = {n.casefold() for n in existing}
    if name.casefold() not in taken:
        return name
    base = re.sub(r'\s*\(\d+\)$', '', name) or name
    n = 2
    while f'{base} ({n})'.casefold() in taken:
        n += 1
    return f'{base} ({n})'


# Short labels for the summary: several sliders share the Camera Raw label "Amount".
_SHORT_LABELS = {
    'sharpening_amount': 'Sharpening',
    'vignette_amount': 'Vignette',
    'noise_reduction': 'Noise Red.',
}


def describe_settings(s: AdjustmentSettings) -> list[str]:
    """Human summary of the non-default values, e.g. ``["Exposure +0.30", "Contrast +15"]``
    (in the UI language)."""
    out: list[str] = []
    for spec in SLIDERS:
        v = float(getattr(s, spec.key))
        if abs(v - spec.default) < 1e-9:
            continue
        label = tr(_SHORT_LABELS.get(spec.key, spec.label))
        sign = '+' if v > 0 and spec.minimum < 0 else ''
        out.append(f'{label} {sign}{v:.{spec.decimals}f}')
    if s.super_resolution != 'off':
        out.append(tr('Super Res {factor}', factor=s.super_resolution))
    if not s.image_size.is_default():
        unit = '%' if s.image_size.mode == 'percent' else ' px'
        out.append(
            tr(
                'Resize {mode} {value}',
                mode=tr(IMAGE_SIZE_MODES[s.image_size.mode]),
                value=f'{s.image_size.value:g}{unit}',
            )
        )
    return out
