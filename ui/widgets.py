"""Small shared widgets: logo mark, page header, badges, icon buttons."""

from __future__ import annotations

from PySide6.QtCore import QPointF, QRectF, QSize, Qt
from PySide6.QtGui import QColor, QLinearGradient, QPainter, QPainterPath, QPaintEvent, QPen
from PySide6.QtWidgets import QHBoxLayout, QLabel, QPushButton, QSizePolicy, QToolButton, QVBoxLayout, QWidget

from ui.icons import icon

ACCENT_STOPS = ((0.0, '#22d3ee'), (0.5, '#4f8cff'), (1.0, '#8b5cf6'))


class ElidedLabel(QLabel):
    """Label that elides long text (paths, summaries) instead of growing the window."""

    def __init__(
        self,
        text: str = '',
        parent: QWidget | None = None,
        mode: Qt.TextElideMode = Qt.TextElideMode.ElideMiddle,
    ) -> None:
        super().__init__(parent)
        self._full = text
        self._mode = mode
        self.setSizePolicy(QSizePolicy.Policy.Ignored, QSizePolicy.Policy.Preferred)
        self.setText(text)

    def setText(self, text: str) -> None:  # noqa: N802
        self._full = text
        if not self.toolTip() or self.toolTip() == getattr(self, '_auto_tip', None):
            self._auto_tip = text
            super().setToolTip(text)
        self._elide()

    def full_text(self) -> str:
        return self._full

    def _elide(self) -> None:
        metrics = self.fontMetrics()
        super().setText(metrics.elidedText(self._full, self._mode, max(40, self.width())))

    def resizeEvent(self, e) -> None:  # noqa: N802, ANN001
        super().resizeEvent(e)
        self._elide()


def paint_logo(p: QPainter, rect: QRectF) -> None:
    """Gradient rounded square with an aperture-like photo glyph."""
    p.save()
    p.setRenderHint(QPainter.RenderHint.Antialiasing)
    g = QLinearGradient(rect.topLeft(), rect.bottomRight())
    for pos, color in ACCENT_STOPS:
        g.setColorAt(pos, QColor(color))
    p.setPen(Qt.PenStyle.NoPen)
    p.setBrush(g)
    r = rect.width() * 0.26
    p.drawRoundedRect(rect, r, r)
    s = rect.width()
    pen = QPen(QColor('#ffffff'), max(1.5, s * 0.075))
    pen.setCapStyle(Qt.PenCapStyle.RoundCap)
    pen.setJoinStyle(Qt.PenJoinStyle.RoundJoin)
    p.setPen(pen)
    p.setBrush(Qt.BrushStyle.NoBrush)
    frame = QRectF(rect.left() + s * 0.22, rect.top() + s * 0.27, s * 0.56, s * 0.46)
    p.drawRoundedRect(frame, s * 0.07, s * 0.07)
    path = QPainterPath(QPointF(frame.left() + s * 0.02, frame.bottom() - s * 0.06))
    path.lineTo(frame.left() + s * 0.2, frame.top() + s * 0.22)
    path.lineTo(frame.left() + s * 0.32, frame.top() + s * 0.33)
    path.lineTo(frame.left() + s * 0.4, frame.top() + s * 0.26)
    path.lineTo(frame.right() - s * 0.02, frame.bottom() - s * 0.08)
    p.drawPath(path)
    p.setBrush(QColor('#ffffff'))
    p.setPen(Qt.PenStyle.NoPen)
    p.drawEllipse(QPointF(frame.right() - s * 0.13, frame.top() + s * 0.12), s * 0.045, s * 0.045)
    p.restore()


class LogoMark(QWidget):
    def __init__(self, size: int = 36, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setFixedSize(size, size)

    def paintEvent(self, _e: QPaintEvent) -> None:  # noqa: N802
        p = QPainter(self)
        paint_logo(p, QRectF(0.5, 0.5, self.width() - 1, self.height() - 1))


def badge(text: str, kind: str = '') -> QLabel:
    lab = QLabel(text)
    lab.setObjectName('Badge')
    if kind:
        lab.setProperty('kind', kind)
    lab.setSizePolicy(QSizePolicy.Policy.Fixed, QSizePolicy.Policy.Fixed)
    return lab


def set_badge(lab: QLabel, text: str, kind: str = '') -> None:
    lab.setText(text)
    lab.setProperty('kind', kind)
    lab.style().unpolish(lab)
    lab.style().polish(lab)


def button(
    text: str,
    icon_name: str | None = None,
    object_name: str = '',
    tooltip: str = '',
    icon_color: str | None = None,
) -> QPushButton:
    b = QPushButton(text)
    if object_name:
        b.setObjectName(object_name)
    if icon_name:
        light = object_name in ('Accent', 'Primary', 'RunButton', 'Danger')
        b.setIcon(icon(icon_name, icon_color or ('#ffffff' if light else '#9fb0cc')))
        b.setIconSize(QSize(16, 16))
    if tooltip:
        b.setToolTip(tooltip)
    b.setCursor(Qt.CursorShape.PointingHandCursor)
    return b


def tool_button(icon_name: str, tooltip: str, object_name: str = 'Ghost') -> QToolButton:
    b = QToolButton()
    b.setObjectName(object_name)
    b.setIcon(icon(icon_name))
    b.setIconSize(QSize(18, 18))
    b.setToolTip(tooltip)
    b.setCursor(Qt.CursorShape.PointingHandCursor)
    return b


class PageHeader(QWidget):
    """Big title + subtitle on the left, an optional widget row on the right."""

    def __init__(self, title: str, subtitle: str = '', parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.title = QLabel(title)
        self.title.setObjectName('PageTitle')
        self.subtitle = QLabel(subtitle)
        self.subtitle.setObjectName('PageSubtitle')
        self.subtitle.setVisible(bool(subtitle))
        col = QVBoxLayout()
        col.setSpacing(2)
        col.addWidget(self.title)
        col.addWidget(self.subtitle)
        self.right = QHBoxLayout()
        self.right.setSpacing(8)
        lay = QHBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.addLayout(col, 1)
        lay.addLayout(self.right)
