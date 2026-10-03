"""Adjustment settings, parameter limits and JSON read/write (spec sections 5.2, 5.5, 5.6)."""

from __future__ import annotations

import json
import logging
import math
from dataclasses import dataclass, field, fields
from pathlib import Path
from typing import Any

log = logging.getLogger(__name__)

MAX_OUTPUT_LONG_EDGE = 20_000  # px
MAX_OUTPUT_MEGAPIXELS = 200  # MP

PRESET_FORMAT_VERSION = 2


@dataclass(frozen=True)
class SliderSpec:
    """Range and UI metadata for one numeric adjustment (Camera Raw naming)."""

    key: str
    label: str
    panel: str  # "Basic" | "Detail" | "Effects"
    group: str  # sub-group inside the panel, e.g. "White Balance"
    minimum: float
    maximum: float
    step: float
    default: float = 0.0
    decimals: int = 0


SLIDERS: tuple[SliderSpec, ...] = (
    SliderSpec('temperature', 'Temperature', 'Basic', 'White Balance', -100, 100, 1),
    SliderSpec('tint', 'Tint', 'Basic', 'White Balance', -100, 100, 1),
    SliderSpec('exposure', 'Exposure', 'Basic', 'Tone', -5.0, 5.0, 0.05, decimals=2),
    SliderSpec('brightness', 'Brightness', 'Basic', 'Tone', -100, 100, 1),
    SliderSpec('contrast', 'Contrast', 'Basic', 'Tone', -100, 100, 1),
    SliderSpec('highlights', 'Highlights', 'Basic', 'Tone', -100, 100, 1),
    SliderSpec('shadows', 'Shadows', 'Basic', 'Tone', -100, 100, 1),
    SliderSpec('whites', 'Whites', 'Basic', 'Tone', -100, 100, 1),
    SliderSpec('blacks', 'Blacks', 'Basic', 'Tone', -100, 100, 1),
    SliderSpec('clarity', 'Clarity', 'Basic', 'Presence', -100, 100, 1),
    SliderSpec('vibrance', 'Vibrance', 'Basic', 'Presence', -100, 100, 1),
    SliderSpec('saturation', 'Saturation', 'Basic', 'Presence', -100, 100, 1),
    SliderSpec('sharpening_amount', 'Amount', 'Detail', 'Sharpening', 0, 150, 1),
    SliderSpec('noise_reduction', 'Noise Reduction', 'Detail', 'Noise Reduction', 0, 100, 1),
    SliderSpec('vignette_amount', 'Amount', 'Effects', 'Post-Crop Vignetting', -100, 100, 1),
)
SLIDER_BY_KEY: dict[str, SliderSpec] = {s.key: s for s in SLIDERS}

SUPER_RESOLUTION_OPTIONS: tuple[str, ...] = ('off', '2x', '4x')
SUPER_RESOLUTION_FACTORS: dict[str, int] = {'off': 1, '2x': 2, '4x': 4}

IMAGE_SIZE_MODES: dict[str, str] = {
    'off': 'Off',
    'percent': 'Percent',
    'long_edge': 'Long Edge',
    'width': 'Width',
    'height': 'Height',
}
RESAMPLE_OPTIONS: dict[str, str] = {
    'automatic': 'Automatic',
    'preserve_details': 'Preserve Details (enlargement)',
    'bicubic_smoother': 'Bicubic Smoother (enlargement)',
    'bicubic_sharper': 'Bicubic Sharper (reduction)',
    'bicubic': 'Bicubic (smooth gradients)',
    'bilinear': 'Bilinear',
    'nearest_neighbor': 'Nearest Neighbor (hard edges)',
}
PERCENT_RANGE = (1.0, 400.0)
PIXEL_RANGE = (16.0, 20_000.0)


def _clamp(value: float, lo: float, hi: float) -> float:
    return max(lo, min(hi, value))


def _as_float(value: Any, default: float) -> float:
    """Convert to a finite float, falling back to ``default``."""
    if isinstance(value, bool):
        return default
    try:
        f = float(value)
    except (TypeError, ValueError):
        return default
    return f if math.isfinite(f) else default


@dataclass
class ImageSizeSettings:
    """Photoshop Image › Image Size options (section 5.5)."""

    mode: str = 'off'
    value: float = 100
    resample: str = 'automatic'
    dont_enlarge: bool = False

    def value_range(self) -> tuple[float, float]:
        return PERCENT_RANGE if self.mode == 'percent' else PIXEL_RANGE

    def is_default(self) -> bool:
        return self.mode == 'off'

    def to_dict(self) -> dict[str, Any]:
        value: float | int = self.value
        if float(value).is_integer():
            value = int(value)
        return {
            'mode': self.mode,
            'value': value,
            'resample': self.resample,
            'dont_enlarge': bool(self.dont_enlarge),
        }

    @classmethod
    def from_dict(cls, data: Any) -> ImageSizeSettings:
        """Build from untrusted data: invalid enums → default, value clamped to the mode's range."""
        s = cls()
        if not isinstance(data, dict):
            return s
        mode = data.get('mode', s.mode)
        s.mode = mode if mode in IMAGE_SIZE_MODES else 'off'
        resample = data.get('resample', s.resample)
        s.resample = resample if resample in RESAMPLE_OPTIONS else 'automatic'
        dont = data.get('dont_enlarge', False)
        s.dont_enlarge = dont if isinstance(dont, bool) else False
        lo, hi = s.value_range()
        default_value = 100.0 if s.mode in ('off', 'percent') else 2048.0
        s.value = _clamp(_as_float(data.get('value', default_value), default_value), lo, hi)
        return s


@dataclass
class AdjustmentSettings:
    """All 17 adjustments. Default values leave the image unchanged."""

    temperature: float = 0
    tint: float = 0
    exposure: float = 0
    brightness: float = 0
    contrast: float = 0
    highlights: float = 0
    shadows: float = 0
    whites: float = 0
    blacks: float = 0
    clarity: float = 0
    vibrance: float = 0
    saturation: float = 0
    sharpening_amount: float = 0
    noise_reduction: float = 0
    vignette_amount: float = 0
    super_resolution: str = 'off'
    image_size: ImageSizeSettings = field(default_factory=ImageSizeSettings)

    @property
    def sr_factor(self) -> int:
        return SUPER_RESOLUTION_FACTORS.get(self.super_resolution, 1)

    def is_default(self) -> bool:
        return (
            all(getattr(self, s.key) == s.default for s in SLIDERS)
            and self.super_resolution == 'off'
            and self.image_size.is_default()
        )

    def copy(self) -> AdjustmentSettings:
        return AdjustmentSettings.from_dict(self.to_dict())

    def to_dict(self) -> dict[str, Any]:
        out: dict[str, Any] = {}
        for spec in SLIDERS:
            v = getattr(self, spec.key)
            out[spec.key] = int(v) if float(v).is_integer() else round(float(v), 4)
        out['super_resolution'] = self.super_resolution
        out['image_size'] = self.image_size.to_dict()
        return out

    @classmethod
    def from_dict(cls, data: Any) -> AdjustmentSettings:
        """Build from untrusted data (section 8.9).

        Missing keys → default, unknown keys → ignored, out-of-range → clamped,
        invalid enum values → default.
        """
        s = cls()
        if not isinstance(data, dict):
            return s
        for spec in SLIDERS:
            if spec.key in data:
                v = _as_float(data[spec.key], spec.default)
                setattr(s, spec.key, _clamp(v, spec.minimum, spec.maximum))
        sr = data.get('super_resolution', 'off')
        s.super_resolution = sr if sr in SUPER_RESOLUTION_OPTIONS else 'off'
        s.image_size = ImageSizeSettings.from_dict(data.get('image_size'))
        return s


def setting_keys() -> list[str]:
    """All top-level setting keys, in Camera Raw order."""
    return [f.name for f in fields(AdjustmentSettings)]


def save_settings_json(path: Path, settings: AdjustmentSettings, name: str = '') -> None:
    """Write settings in the preset file format (section 8.9)."""
    payload = {'version': PRESET_FORMAT_VERSION, 'name': name, 'settings': settings.to_dict()}
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + '.tmp')
    tmp.write_text(json.dumps(payload, indent=2, ensure_ascii=False), encoding='utf-8')
    tmp.replace(path)


def load_settings_json(path: Path) -> tuple[str, AdjustmentSettings]:
    """Read a preset/settings file. Returns ``(name, settings)``."""
    data = json.loads(Path(path).read_text(encoding='utf-8'))
    if not isinstance(data, dict):
        raise ValueError(f'Invalid settings file: {path}')
    raw = data.get('settings', data)
    name = data.get('name')
    if not isinstance(name, str):
        name = Path(path).stem
    return name, AdjustmentSettings.from_dict(raw)
