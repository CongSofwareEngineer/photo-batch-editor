"""Completion notice: banner, sound, taskbar flash, Windows notification (spec section 8.4)."""

from __future__ import annotations

import logging
import sys
from pathlib import Path

from PySide6.QtCore import QRectF, Qt, Signal
from PySide6.QtGui import QFont, QIcon, QPainter, QPixmap
from PySide6.QtWidgets import QApplication, QFrame, QHBoxLayout, QLabel, QSystemTrayIcon, QVBoxLayout, QWidget

from core.batch import BatchReport
from core.i18n import tr, trn
from ui.widgets import button, paint_logo

log = logging.getLogger(__name__)


def device_badge(report: BatchReport) -> str:
    return {'gpu': '⚡ GPU', 'cpu': 'CPU', 'mixed': 'GPU + CPU'}.get(report.device_used, report.device_used)


def format_duration(seconds: float) -> str:
    seconds = int(round(seconds))
    if seconds < 60:
        return tr('{s} s', s=seconds)
    m, s = divmod(seconds, 60)
    if m < 60:
        return tr('{m} min {s} s', m=m, s=s)
    h, m = divmod(m, 60)
    return tr('{h} h {m} min', h=h, m=m)


def banner_for_report(report: BatchReport) -> tuple[str, str]:
    """``(kind, text)``: kind is success / warning / error / neutral (section 8.4 table)."""
    total = len(report.files)
    ok = report.succeeded
    warn = report.count('warning')
    err = report.count('error')
    dev = device_badge(report)
    if report.cancelled:
        return 'neutral', tr('■ Cancelled: {ok}/{total} photos edited · {dev}', ok=ok, total=total, dev=dev)
    if err:
        return 'error', tr(
            '❌ Finished: {ok}/{total} succeeded, {err} failed · {dev}', ok=ok, total=total, err=err, dev=dev
        )
    if warn:
        return 'warning', tr(
            '⚠ Done: {ok}/{total} photos, {warn} with warnings · {dev}',
            ok=ok,
            total=total,
            warn=warn,
            dev=dev,
        )
    return 'success', tr(
        '✅ Done: {ok}/{total} photos edited in {time} · {dev}',
        ok=ok,
        total=total,
        time=format_duration(report.total_seconds),
        dev=dev,
    )


class Banner(QFrame):
    """Result banner at the top of Screen 3 (not a modal popup)."""

    open_folder = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName('Banner')
        self.text = QLabel()
        f = QFont(self.text.font())
        f.setPointSize(13)
        f.setBold(True)
        self.text.setFont(f)
        self.path = QLabel()
        self.path.setProperty('secondary', True)
        self.path.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        self.button = button(tr('Open Folder'), 'folder')
        col = QVBoxLayout()
        col.setSpacing(2)
        col.addWidget(self.text)
        col.addWidget(self.path)
        lay = QHBoxLayout(self)
        lay.setContentsMargins(14, 10, 14, 10)
        lay.addLayout(col, 1)
        lay.addWidget(self.button, 0, Qt.AlignmentFlag.AlignVCenter)
        self.button.clicked.connect(self.open_folder.emit)

    def show_report(self, report: BatchReport) -> None:
        kind, text = banner_for_report(report)
        self.text.setText(text)
        self.path.setText(str(report.output_dir))
        self.setProperty('kind', kind)
        self.style().unpolish(self)
        self.style().polish(self)


def app_icon() -> QIcon:
    """Generated gradient logo (no image files needed)."""
    ic = QIcon()
    for size in (16, 24, 32, 48, 64, 128, 256):
        pm = QPixmap(size, size)
        pm.fill(Qt.GlobalColor.transparent)
        p = QPainter(pm)
        m = size / 32
        paint_logo(p, QRectF(m, m, size - 2 * m, size - 2 * m))
        p.end()
        ic.addPixmap(pm)
    return ic


class Notifier:
    """Sound + taskbar flash + tray notification when a batch ends."""

    def __init__(self, window: QWidget) -> None:
        self.window = window
        self.tray: QSystemTrayIcon | None = None
        if QSystemTrayIcon.isSystemTrayAvailable():
            self.tray = QSystemTrayIcon(app_icon(), window)
            self.tray.setToolTip('Photo Batch Editor')
            self.tray.messageClicked.connect(self.bring_to_front)
            self.tray.activated.connect(lambda _reason: self.bring_to_front())

    def bring_to_front(self) -> None:
        w = self.window
        if w.isMinimized():
            w.showNormal()
        w.show()
        w.raise_()
        w.activateWindow()

    def _play_sound(self) -> None:
        if sys.platform == 'win32':
            try:
                import winsound  # noqa: PLC0415

                winsound.MessageBeep(winsound.MB_ICONASTERISK)
                return
            except Exception:  # noqa: BLE001
                pass
        QApplication.beep()

    def notify(self, report: BatchReport) -> None:
        self._play_sound()
        w = self.window
        if not w.isActiveWindow():
            QApplication.alert(w, 0)  # flash the taskbar button until focused
        if self.tray is not None and (w.isMinimized() or not w.isActiveWindow()):
            name = Path(report.input_dir).name
            ok, total = report.succeeded, len(report.files)
            if report.cancelled:
                msg = tr('Cancelled: {ok}/{total} photos of {name} edited', ok=ok, total=total, name=name)
            elif report.count('error'):
                msg = tr(
                    '{ok}/{total} photos of {name} done, {err} failed',
                    ok=ok,
                    total=total,
                    name=name,
                    err=report.count('error'),
                )
            else:
                msg = trn('{n} photo of {name} is done', '{n} photos of {name} are done', ok, name=name)
            self.tray.show()
            self.tray.showMessage('Photo Batch Editor', msg, app_icon(), 10_000)
