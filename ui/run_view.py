"""Screen 2 — Running (spec section 8.3)."""

from __future__ import annotations

import time
from pathlib import Path

from PySide6.QtCore import QTimer, Signal
from PySide6.QtGui import QColor
from PySide6.QtWidgets import (
    QHBoxLayout,
    QLabel,
    QListWidget,
    QListWidgetItem,
    QProgressBar,
    QVBoxLayout,
    QWidget,
)

from core.backend import Backend
from core.batch import FileResult
from core.i18n import tr, tr_msg
from ui.widgets import button

STATUS_MARK = {
    'ok': ('✓', '#34d399'),
    'warning': ('⚠', '#fbbf24'),
    'error': ('✗', '#f87171'),
    'cancelled': ('■', '#8d9bb8'),
}


def format_eta(seconds: float) -> str:
    seconds = max(0, int(round(seconds)))
    m, s = divmod(seconds, 60)
    return tr('~{m} min {s} s', m=m, s=f'{s:02d}')


class RunView(QWidget):
    cancel_requested = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.paths = QLabel()
        self.paths.setObjectName('Heading')
        self.paths.setWordWrap(True)
        self.device = QLabel()
        self.progress = QProgressBar()
        self.progress.setMinimumHeight(28)
        self.count = QLabel()
        self.count.setObjectName('Heading')
        self.current = QLabel()
        self.current.setProperty('secondary', True)
        self.eta = QLabel()
        self.eta.setProperty('secondary', True)
        self.log = QListWidget()
        self.log.setAlternatingRowColors(True)
        self.cancel_btn = button(tr('Cancel'), 'stop', 'Danger')
        self.cancel_btn.setMinimumWidth(120)

        lay = QVBoxLayout(self)
        lay.setContentsMargins(40, 30, 40, 24)
        lay.setSpacing(10)
        lay.addWidget(self.paths)
        lay.addWidget(self.device)
        lay.addSpacing(16)
        prow = QHBoxLayout()
        prow.addWidget(self.progress, 1)
        prow.addWidget(self.count)
        lay.addLayout(prow)
        crow = QHBoxLayout()
        crow.addWidget(self.current, 1)
        crow.addWidget(self.eta)
        lay.addLayout(crow)
        lay.addSpacing(8)
        lay.addWidget(self.log, 1)
        brow = QHBoxLayout()
        brow.addStretch(1)
        brow.addWidget(self.cancel_btn)
        lay.addLayout(brow)

        self.cancel_btn.clicked.connect(self.cancel_requested.emit)
        self._start = 0.0
        self._total = 0
        self._done = 0
        self._backend: Backend | None = None
        self._device_text = ''
        self._fallback = False
        self._vram_timer = QTimer(self)
        self._vram_timer.setInterval(1000)
        self._vram_timer.timeout.connect(self._update_device_line)

    def start(self, input_dir: Path, output_dir: Path, total: int, backend: Backend) -> None:
        self.paths.setText(tr('Editing: {src}  →  {dst}', src=input_dir, dst=output_dir))
        self._backend = backend
        self._device_text = f'⚡ GPU {backend.device_name}' if backend.name == 'gpu' else backend.device_name
        self._fallback = False
        self._start = time.perf_counter()
        self._total, self._done = total, 0
        self.progress.setRange(0, max(1, total))
        self.progress.setValue(0)
        self.count.setText(f'0 / {total}  (0%)')
        self.current.setText(tr('Starting…'))
        self.eta.setText('')
        self.log.clear()
        self.cancel_btn.setEnabled(True)
        self.cancel_btn.setText(tr('Cancel'))
        self._update_device_line()
        if backend.name == 'gpu':
            self._vram_timer.start()

    def stop(self) -> None:
        self._vram_timer.stop()

    def set_cancelling(self) -> None:
        self.cancel_btn.setEnabled(False)
        self.cancel_btn.setText(tr('Cancelling…'))
        self.current.setText(tr('Cancelling: finishing the photo in progress…'))

    def gpu_fallback(self) -> None:
        self._fallback = True
        self._update_device_line()

    def _update_device_line(self) -> None:
        if self._fallback:
            self.device.setText(tr('Process with: ⚠ GPU error — continuing on CPU'))
            self.device.setStyleSheet('color: #fbbf24;')
            return
        self.device.setStyleSheet('')
        text = tr('Process with: {device}', device=self._device_text)
        if self._backend is not None and self._backend.name == 'gpu':
            info = self._backend.mem_info()
            if info:
                free, total = info
                text += f'    (VRAM {(total - free) / 2**30:.1f} / {total / 2**30:.0f} GB)'
        self.device.setText(text)

    def percent(self) -> int:
        return int(100 * self._done / self._total) if self._total else 0

    def on_progress(self, done: int, total: int, current: str, res: FileResult | None) -> None:
        self._done, self._total = done, total
        self.progress.setValue(done)
        self.count.setText(f'{done} / {total}  ({self.percent()}%)')
        if res is None:
            if not self.cancel_btn.isEnabled():
                return
            self.current.setText(tr('Processing: {name}', name=current))
        else:
            mark, color = STATUS_MARK.get(res.status, ('•', '#e7ecf6'))
            name = res.output_path.name if res.output_path else res.input_path.name
            rel = f'{res.rel_dir}/' if res.rel_dir else ''
            text = f'{mark} {rel}{name}' + (f'  ({tr_msg(res.message)})' if res.message else '')
            item = QListWidgetItem(text)
            item.setForeground(QColor(color))
            self.log.addItem(item)
            self.log.scrollToBottom()
        elapsed = time.perf_counter() - self._start
        if done:
            self.eta.setText(tr('Time left: {eta}', eta=format_eta(elapsed / done * (total - done))))
