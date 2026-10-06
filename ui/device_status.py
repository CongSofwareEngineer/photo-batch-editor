"""“Process with” selector and the GPU/CPU status chip (spec sections 10.1–10.2)."""

from __future__ import annotations

from typing import cast

from PySide6.QtCore import Qt, Signal
from PySide6.QtGui import QStandardItemModel
from PySide6.QtWidgets import QComboBox, QHBoxLayout, QLabel, QWidget

from core.backend import GpuInfo
from core.batch import default_cpu_workers
from core.i18n import tr, tr_msg, trn
from core.settings import AdjustmentSettings

GPU_LABEL = '⚡ NVIDIA GPU (recommended)'  # translated when shown
CPU_LABEL = 'CPU'


def _gb(n: int | None) -> str:
    return f'{(n or 0) / 2**30:.1f} GB'


def gpu_tooltip(info: GpuInfo | None) -> str:
    if info is None:
        return tr('Detecting NVIDIA GPU…')
    if not info.available:
        # No usable NVIDIA GPU is normal (laptop, AMD/Intel graphics, no driver): the app simply
        # runs on the CPU, nothing to install. The reason is kept as a detail for support.
        lines = [
            '<b>' + tr('No usable NVIDIA GPU — the app runs on the CPU. Nothing to install.') + '</b>',
            '',
            tr('Details: ') + (tr_msg(info.reason or '') or tr('Unknown reason.')),
        ]
        if info.name:
            lines.insert(1, f'GPU: {info.name}')
        return '<br>'.join(lines)
    return '<br>'.join(
        [
            f'<b>{info.name}</b>',
            tr('VRAM: {free} free / {total} total', free=_gb(info.vram_free), total=_gb(info.vram_total)),
            tr('Driver: {version}', version=info.driver_version or tr('unknown')),
            tr('CUDA runtime: {version}', version=info.cuda_version or tr('unknown')),
            tr('Super Resolution: ')
            + (
                'CUDA (GPU)' if info.sr_cuda_available else tr('CPU (ONNX Runtime CUDA provider unavailable)')
            ),
        ]
    )


class StatusChip(QLabel):
    """Always-visible chip: gray / green / yellow (section 10.2)."""

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName('Chip')
        self.set_state('gray', tr('⏳ Detecting GPU…'), tr('Detecting NVIDIA GPU…'))

    def set_state(self, state: str, text: str, tooltip: str) -> None:
        self.setText(text)
        self.setToolTip(tooltip)
        self.setProperty('state', state)
        self.style().unpolish(self)
        self.style().polish(self)


class DeviceStatus(QWidget):
    """Selector + chip. ``device_changed`` carries ``"gpu"`` or ``"cpu"``."""

    device_changed = Signal(str)

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.info: GpuInfo | None = None
        self.settings = AdjustmentSettings()
        self._preferred = 'gpu'  # the user's choice; the GPU is used only if it is usable
        self._applying = False
        lay = QHBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(8)
        lab = QLabel(tr('Process with:'))
        lab.setProperty('secondary', True)
        self.combo = QComboBox()
        self.combo.addItem(tr(GPU_LABEL), 'gpu')
        self.combo.addItem(CPU_LABEL, 'cpu')
        self.combo.setToolTip(tr('Device used for the live preview and for RUN'))
        self.chip = StatusChip()
        lay.addWidget(lab)
        lay.addWidget(self.combo)
        lay.addWidget(self.chip)
        self.combo.currentIndexChanged.connect(self._changed)

    # API
    def device(self) -> str:
        return self.combo.currentData()

    def preferred_device(self) -> str:
        """Device chosen by the user (saved), even if the GPU is unusable on this computer."""
        return self._preferred

    def set_device(self, device: str) -> None:
        self._preferred = 'cpu' if device == 'cpu' else 'gpu'
        self._apply()
        self.refresh()

    def set_gpu_info(self, info: GpuInfo) -> None:
        self.info = info
        self._apply()
        self.refresh()

    def _apply(self) -> None:
        """Select CPU silently when the GPU is unusable (the GPU item is disabled)."""
        gpu_ok = self.info is None or self.info.available
        item = cast(QStandardItemModel, self.combo.model()).item(0)
        if item is not None:
            item.setEnabled(gpu_ok)
        self.combo.setItemData(
            0, None if gpu_ok else tr('No usable NVIDIA GPU on this computer'), Qt.ItemDataRole.ToolTipRole
        )
        self._applying = True
        try:
            self.combo.setCurrentIndex(0 if gpu_ok and self._preferred == 'gpu' else 1)
        finally:
            self._applying = False

    def set_settings(self, settings: AdjustmentSettings) -> None:
        self.settings = settings
        self.refresh()

    def effective_device(self) -> str:
        """Device that will actually run: GPU only if selected and usable."""
        return 'gpu' if self.device() == 'gpu' and self.info and self.info.available else 'cpu'

    def refresh(self) -> None:
        if self.device() == 'cpu':
            n = default_cpu_workers(self.settings)
            self.chip.set_state(
                'gray',
                'CPU · ' + trn('{n} process', '{n} processes', n),
                tr('Processing on the CPU.') + '<br><br>' + gpu_tooltip(self.info),
            )
        elif self.info is None:
            self.chip.set_state('gray', tr('⏳ Detecting GPU…'), gpu_tooltip(None))
        elif self.info.available:
            self.chip.set_state(
                'green',
                f'⚡ GPU: {self.info.name} · {_gb(self.info.vram_total).replace(".0 GB", " GB")}',
                gpu_tooltip(self.info),
            )
        else:
            self.chip.set_state('yellow', tr('⚠ GPU unavailable — using CPU'), gpu_tooltip(self.info))

    def _changed(self) -> None:
        if not self._applying:
            self._preferred = self.device()
        self.refresh()
        self.device_changed.emit(self.device())
