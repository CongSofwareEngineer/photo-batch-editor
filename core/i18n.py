"""UI language (English / Vietnamese).

The English text itself is the key: ``tr("Choose Folder")`` returns it unchanged in English
and looks it up in :mod:`core.i18n_vi` in Vietnamese (missing entries fall back to English).
Placeholders use ``str.format``: ``tr("Folder not found: {path}", path=p)``.

``core`` keeps producing English messages (tests, ``process_log.txt``); the UI translates
them when they are shown with :func:`tr_msg`.
"""

from __future__ import annotations

import json
import logging
import re
from pathlib import Path

log = logging.getLogger(__name__)

LANGUAGES: dict[str, str] = {'en': 'English', 'vi': 'Tiếng Việt'}
DEFAULT_LANGUAGE = 'en'
PREFS_FILE = 'preferences.json'

_lang = DEFAULT_LANGUAGE
_catalog: dict[str, str] = {}
_patterns: list[tuple[re.Pattern[str], str]] = []


def language() -> str:
    return _lang


def set_language(code: str) -> None:
    global _lang, _catalog, _patterns
    _lang = code if code in LANGUAGES else DEFAULT_LANGUAGE
    if _lang == 'vi':
        from core import i18n_vi  # noqa: PLC0415

        _catalog = i18n_vi.TEXT
        _patterns = [(re.compile(p), t) for p, t in i18n_vi.MESSAGE_PATTERNS]
    else:
        _catalog, _patterns = {}, []


def tr(text: str, **kwargs: object) -> str:
    s = _catalog.get(text, text)
    return s.format(**kwargs) if kwargs else s


def trn(singular: str, plural: str, n: int, **kwargs: object) -> str:
    """Plural-aware ``tr``; ``{n}`` is filled in. The catalog is keyed by the plural form."""
    key = plural if n != 1 else singular
    s = _catalog.get(plural, key) if _catalog else key
    return s.format(n=n, **kwargs)


def tr_msg(message: str) -> str:
    """Translate a message coming from ``core`` (warnings joined with ``"; "``, errors with details)."""
    if not message or not _catalog:
        return message
    return '; '.join(_tr_one(part) for part in message.split('; '))


def _tr_one(part: str) -> str:
    if part in _catalog:
        return _catalog[part]
    for pattern, template in _patterns:
        m = pattern.match(part)
        if m:
            return template.format(*(_tr_one(g) if g else g for g in m.groups()))
    return part


# Preference file --------------------------------------------------------------------------


def system_language() -> str:
    try:
        from PySide6.QtCore import QLocale  # noqa: PLC0415

        return 'vi' if QLocale.system().language() == QLocale.Language.Vietnamese else 'en'
    except Exception:  # noqa: BLE001
        return DEFAULT_LANGUAGE


def load_language(data_dir: Path) -> str:
    """Saved language, or the Windows display language the first time."""
    try:
        code = json.loads((Path(data_dir) / PREFS_FILE).read_text(encoding='utf-8')).get('language')
    except (OSError, ValueError, AttributeError):
        code = None
    return code if code in LANGUAGES else system_language()


def save_language(data_dir: Path, code: str) -> None:
    path = Path(data_dir) / PREFS_FILE
    try:
        prefs = json.loads(path.read_text(encoding='utf-8'))
        if not isinstance(prefs, dict):
            prefs = {}
    except (OSError, ValueError):
        prefs = {}
    prefs['language'] = code
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        tmp = path.with_name(path.name + '.tmp')
        tmp.write_text(json.dumps(prefs, indent=2), encoding='utf-8')
        tmp.replace(path)
    except OSError:
        log.warning('Could not save the language', exc_info=True)
