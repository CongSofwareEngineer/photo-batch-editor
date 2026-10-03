"""UI language: every text of the app has a Vietnamese entry with the same placeholders."""

import ast
import re
import string
from pathlib import Path

import pytest

from core import i18n
from core.backend import NO_GPU, OLD_DRIVER
from core.batch import CORRUPT_FILE, GPU_ERROR_WARNING, GPU_FALLBACK_WARNING
from core.enhance import SR_SKIP_WARNING
from core.i18n import load_language, save_language, set_language, tr, tr_msg, trn
from core.i18n_vi import MESSAGE_PATTERNS, TEXT
from core.image_size import CLAMP_WARNING
from core.io_utils import EXIF_NOT_KEPT
from core.presets import _SHORT_LABELS, list_builtin_presets
from core.settings import IMAGE_SIZE_MODES, RESAMPLE_OPTIONS, SLIDERS

ROOT = Path(__file__).resolve().parent.parent


@pytest.fixture(autouse=True)
def _english_afterwards():
    yield
    set_language('en')


def literal_keys() -> dict[str, str]:
    """First argument(s) of every ``tr(...)`` / ``trn(...)`` call written as a literal."""
    keys: dict[str, str] = {}
    for f in [*ROOT.glob('core/**/*.py'), *ROOT.glob('ui/**/*.py')]:
        for node in ast.walk(ast.parse(f.read_text(encoding='utf-8'))):
            if (
                isinstance(node, ast.Call)
                and isinstance(node.func, ast.Name)
                and node.func.id in ('tr', 'trn')
            ):
                for a in node.args[: 2 if node.func.id == 'trn' else 1]:
                    if isinstance(a, ast.Constant) and isinstance(a.value, str):
                        keys.setdefault(a.value, f'{f.name}:{node.lineno}')
    return keys


def indirect_keys() -> set[str]:
    """Texts passed to ``tr`` through variables (tables, constants, preset names)."""
    from ui.adjustment_panel import SR_TIP
    from ui.device_status import GPU_LABEL
    from ui.results_view import COLUMNS, STATUS_TEXT
    from ui.sidebar import PAGES

    keys = {s.label for s in SLIDERS} | {s.group for s in SLIDERS} | {s.panel for s in SLIDERS}
    keys |= set(IMAGE_SIZE_MODES.values()) | set(RESAMPLE_OPTIONS.values()) | set(_SHORT_LABELS.values())
    keys |= set(COLUMNS) | {
        STATUS_TEXT['ok'],
        STATUS_TEXT['cancelled'],
        GPU_LABEL,
        SR_TIP,
        'USER NAME',
        'PASSWORD',
    }
    keys |= {text for _k, text, _i, _t in PAGES} | {tip for _k, _t, _i, tip in PAGES}
    keys |= {p.name for p in list_builtin_presets()}
    keys |= {
        NO_GPU,
        OLD_DRIVER,
        CORRUPT_FILE,
        GPU_ERROR_WARNING,
        GPU_FALLBACK_WARNING,
        SR_SKIP_WARNING,
        CLAMP_WARNING,
        EXIF_NOT_KEPT,
    }
    keys |= photo_video_keys()
    return keys


def photo_video_keys() -> set[str]:
    """Label tables of the photo and video editors."""
    from core.photo.collage import TEMPLATE_LABELS
    from core.photo.effects import ADJUST_SLIDERS
    from core.video.export import SAME_AS_SOURCE
    from core.video.ffmpeg import FFMPEG_NOT_FOUND
    from ui.photo.collage_dialog import IMAGE_FILTER
    from ui.photo.document import HISTORY_LABELS
    from ui.photo.panels import BLUR_MODE_LABELS, BLUR_SHAPES, CropProps
    from ui.photo.photo_editor_view import PROJECT_FILTER, TOOLS
    from ui.video.panels import RESOLUTION_LABELS, TAB_KEYS
    from ui.video.video_editor_view import AUDIO_FILTER, VIDEO_FILTER

    keys = set(TEMPLATE_LABELS.values()) | {s.label for s in ADJUST_SLIDERS} | set(HISTORY_LABELS)
    keys |= {label for _k, label in BLUR_SHAPES} | set(BLUR_MODE_LABELS.values())
    keys |= set(CropProps.RATIO_LABELS.values()) | set(RESOLUTION_LABELS.values())
    keys |= {label for _k, label in TAB_KEYS}
    keys |= {label for _k, label, _i, _s, _t in TOOLS} | {tip for _k, _l, _i, _s, tip in TOOLS}
    keys |= {IMAGE_FILTER, PROJECT_FILTER, VIDEO_FILTER, AUDIO_FILTER, FFMPEG_NOT_FOUND, SAME_AS_SOURCE}
    return keys


def fields(s: str) -> set[str]:
    return {f for _t, f, _s, _c in string.Formatter().parse(s) if f is not None}


def test_every_text_has_a_vietnamese_entry():
    missing = {k: where for k, where in literal_keys().items() if k not in TEXT}
    missing |= {k: 'indirect' for k in indirect_keys() if k not in TEXT}
    assert not missing, '\n'.join(f'{w}: {k!r}' for k, w in missing.items())


def test_placeholders_match():
    bad = [k for k, v in TEXT.items() if fields(k) != fields(v)]
    assert not bad, bad


def test_patterns_compile_and_fill():
    for pattern, template in MESSAGE_PATTERNS:
        n = re.compile(pattern).groups
        assert fields(template) == {str(i) for i in range(n)}, pattern


def test_english_is_identity():
    set_language('en')
    assert tr('Choose Folder') == 'Choose Folder'
    assert tr('Folder not found: {path}', path='X') == 'Folder not found: X'
    assert trn('{n} photo', '{n} photos', 1) == '1 photo'
    assert trn('{n} photo', '{n} photos', 3) == '3 photos'
    assert tr_msg('Processing failed: boom') == 'Processing failed: boom'


def test_vietnamese():
    set_language('vi')
    assert tr('Choose Folder') == 'Chọn thư mục'
    assert tr('Folder not found: {path}', path='X') == 'Không tìm thấy thư mục: X'
    assert trn('{n} photo', '{n} photos', 1) == '1 ảnh'
    assert tr('not in the catalog') == 'not in the catalog'
    assert (
        tr_msg(f'{GPU_FALLBACK_WARNING}; {EXIF_NOT_KEPT}')
        == 'Ảnh quá lớn so với VRAM, đã xử lý bằng CPU; Không giữ được EXIF'
    )
    assert tr_msg('Processing failed: boom') == 'Xử lý thất bại: boom'
    assert tr_msg('Saved as a (2).jpg (name collision)') == 'Đã lưu thành a (2).jpg (trùng tên)'
    assert (
        tr_msg('Super Resolution skipped: Super Resolution model not found: m.onnx')
        == 'Bỏ qua Super Resolution: Không tìm thấy model Super Resolution: m.onnx'
    )
    assert tr_msg('something unexpected') == 'something unexpected'


def test_describe_settings_follows_language():
    from core.presets import describe_settings
    from core.settings import AdjustmentSettings

    s = AdjustmentSettings(exposure=0.3)
    set_language('vi')
    assert describe_settings(s) == ['Phơi sáng +0.30']
    set_language('en')
    assert describe_settings(s) == ['Exposure +0.30']


def test_language_preference_roundtrip(tmp_path):
    save_language(tmp_path, 'vi')
    assert load_language(tmp_path) == 'vi'
    save_language(tmp_path, 'en')
    assert load_language(tmp_path) == 'en'
    (tmp_path / i18n.PREFS_FILE).write_text('{broken', encoding='utf-8')
    assert load_language(tmp_path) in i18n.LANGUAGES
    set_language('xx')
    assert i18n.language() == 'en'
