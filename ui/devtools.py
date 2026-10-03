"""Dev mode (``dev.bat`` / ``python dev.py``): live reload while editing the source.

* ``ui/theme.qss`` changed → the style sheet is re-applied in place (instant, no restart).
* any ``.py`` file of the app changed → the window closes normally (state is saved) and the
  process exits with :data:`RELOAD_EXIT_CODE`; ``dev.py`` then starts it again.
* the window position/size, the open page and the Settings tab survive a reload, and the
  title bar shows ``[DEV]`` plus the files that triggered the last reload.

Only active when ``PBE_DEV=1`` (set by ``dev.py``); the packaged app never imports it.
"""

from __future__ import annotations

import json
import logging
import os
from pathlib import Path
from typing import TYPE_CHECKING

from PySide6.QtCore import QByteArray, QObject, QTimer
from PySide6.QtWidgets import QApplication

from core.paths import local_appdata_dir

if TYPE_CHECKING:
    from ui.main_window import MainWindow

log = logging.getLogger(__name__)

RELOAD_EXIT_CODE = 3
ROOT = Path(__file__).resolve().parent.parent
WATCH_DIRS = ('core', 'ui')
SESSION_FILE = local_appdata_dir() / 'dev-session.json'


def enabled() -> bool:
    return os.environ.get('PBE_DEV') == '1'


def source_files() -> list[Path]:
    files = [ROOT / 'main.py']
    for d in WATCH_DIRS:
        files += (ROOT / d).rglob('*.py')
    return files


def snapshot(files: list[Path]) -> dict[Path, float]:
    out = {}
    for f in files:
        try:
            out[f] = f.stat().st_mtime
        except OSError:
            pass
    return out


def changed_files(before: dict[Path, float], after: dict[Path, float]) -> list[str]:
    keys = set(before) | set(after)
    return sorted(str(k.relative_to(ROOT)).replace('\\', '/') for k in keys if before.get(k) != after.get(k))


class DevReloader(QObject):
    """Polls the source files twice a second (no extra dependency, works on any drive)."""

    def __init__(self) -> None:
        super().__init__(QApplication.instance())
        self.window: MainWindow | None = None
        self.reload_pending = False
        self.qss = ROOT / 'ui' / 'theme.qss'
        self._py = snapshot(source_files())
        self._qss = snapshot([self.qss])
        self._reason = ''
        self.timer = QTimer(self)
        self.timer.timeout.connect(self._poll)
        self.timer.start(500)

    def attach(self, window: MainWindow) -> None:
        """Follow ``window`` (the first one, or the one rebuilt after a language change)."""
        first = self.window is None
        self.window = window
        if first:
            self._restore_session()
        window.setWindowTitle(f'{window.windowTitle()}  [DEV — {self._reason}]')

    # Session ---------------------------------------------------------------------------------
    def _restore_session(self) -> None:
        try:
            s = json.loads(SESSION_FILE.read_text(encoding='utf-8'))
        except (OSError, ValueError):
            s = {}
        w = self.window
        if isinstance(s.get('geometry'), str):
            w.restoreGeometry(QByteArray.fromBase64(s['geometry'].encode()))
        if s.get('page') in ('settings', 'photo', 'video') and w.root.currentWidget() is w.shell:
            w.show_page(s['page'])
            if s['page'] == 'settings' and isinstance(s.get('settings_tab'), int):
                w.settings_view.show_tab(s['settings_tab'])
        self._reason = s.pop('reason', None) or 'started'
        try:  # the reason is shown once; a fresh dev.py start says "started"
            SESSION_FILE.write_text(json.dumps(s), encoding='utf-8')
        except OSError:
            pass

    def _save_session(self, reason: str) -> None:
        w = self.window
        page = w.current_page()
        s = {
            'geometry': bytes(w.saveGeometry().toBase64().data()).decode(),
            'page': page,
            'settings_tab': w.settings_view.tabs.currentIndex(),
            'reason': reason,
        }
        try:
            SESSION_FILE.parent.mkdir(parents=True, exist_ok=True)
            SESSION_FILE.write_text(json.dumps(s), encoding='utf-8')
        except OSError:
            log.warning('Could not save the dev session', exc_info=True)

    # Watch -----------------------------------------------------------------------------------
    def _poll(self) -> None:
        qss = snapshot([self.qss])
        if qss != self._qss:
            self._qss = qss
            from ui.main_window import load_stylesheet

            app = QApplication.instance()
            if isinstance(app, QApplication):
                app.setStyleSheet(load_stylesheet())
            log.info('[dev] theme.qss re-applied')
        py = snapshot(source_files())
        if py != self._py and self.window is not None:
            names = changed_files(self._py, py)
            self._py = py
            log.info('[dev] changed: %s -> reloading', ', '.join(names))
            self.timer.stop()
            self.reload_pending = True
            self._save_session('reloaded: ' + ', '.join(names[:3]) + (' …' if len(names) > 3 else ''))
            self.window.close()
            if self.window.isVisible() and not self.window._quit_after_cancel:
                # close refused (batch running, user said No): try again on the next change
                self.reload_pending = False
                self.timer.start(500)
