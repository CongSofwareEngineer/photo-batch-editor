"""Video timeline: ruler, clips (video track), texts and music; In / Out marks; playhead.

* Click / drag on the ruler or empty space: move the playhead.
* Click a clip: select it; drag its left / right edge: trim it.
* Text blocks: drag to move in time, drag an edge to change start / end.
* Ctrl + wheel: zoom, wheel: scroll.
"""

from __future__ import annotations

from PySide6.QtCore import QPointF, QRectF, Qt, Signal
from PySide6.QtGui import QColor, QFont, QMouseEvent, QPainter, QPaintEvent, QPen, QPolygonF, QWheelEvent
from PySide6.QtWidgets import QWidget

from core.i18n import tr
from core.video.project import VideoProject, fmt_time, trim_clip

LEFT = 64  # track labels
RULER_H = 26
VIDEO_H = 54
SUB_H = 30
GAP = 6
EDGE = 7  # px: grab zone of a clip / text edge
STEPS = (0.1, 0.25, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600)
CLIP_COLORS = ('#2f5bd3', '#3d4fb8')
ACCENT = QColor('#22d3ee')


class TimelineWidget(QWidget):
    seek_requested = Signal(float)
    clip_selected = Signal(int)
    text_selected = Signal(int)
    music_selected = Signal()
    edit_started = Signal(str)  # undo label — emitted before the first change of a drag
    edited = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName('Timeline')
        self.setMinimumHeight(RULER_H + VIDEO_H + 2 * SUB_H + 4 * GAP + 8)
        self.setMouseTracking(True)
        self.setFocusPolicy(Qt.FocusPolicy.ClickFocus)
        self.project: VideoProject | None = None
        self.t = 0.0
        self.mark_in: float | None = None
        self.mark_out: float | None = None
        self.sel_clip = -1
        self.sel_text = 0
        self.music_selected_flag = False
        self.pps = 50.0  # pixels per second
        self.view_start = 0.0  # first visible second
        self.fit = True
        self._drag: dict | None = None

    # geometry ------------------------------------------------------------------------------------
    def rows(self) -> dict[str, QRectF]:
        w = self.width()
        y = 0.0
        out = {'ruler': QRectF(0, y, w, RULER_H)}
        y += RULER_H + GAP
        out['video'] = QRectF(0, y, w, VIDEO_H)
        y += VIDEO_H + GAP
        out['text'] = QRectF(0, y, w, SUB_H)
        y += SUB_H + GAP
        out['music'] = QRectF(0, y, w, SUB_H)
        return out

    def x_of(self, t: float) -> float:
        return LEFT + (t - self.view_start) * self.pps

    def t_of(self, x: float) -> float:
        return self.view_start + (x - LEFT) / self.pps

    def _fit(self) -> None:
        d = self.project.duration if self.project else 0.0
        self.pps = max(1e-3, (self.width() - LEFT - 16) / max(d, 0.5))
        self.view_start = 0.0

    def resizeEvent(self, e) -> None:  # noqa: N802, ANN001
        super().resizeEvent(e)
        if self.fit:
            self._fit()

    def set_project(self, p: VideoProject | None) -> None:
        self.project = p
        if p is None:
            self.mark_in = self.mark_out = None
            self.sel_clip, self.sel_text = -1, 0
        elif self.sel_clip >= len(p.clips):
            self.sel_clip = -1
        if self.fit or p is None:
            self.fit = True
            self._fit()
        self.update()

    def refit(self) -> None:
        self.fit = True
        self._fit()
        self.update()

    def set_position(self, t: float) -> None:
        self.t = t
        x = self.x_of(t)
        if not self.fit and (x < LEFT or x > self.width() - 10):
            self.view_start = max(0.0, t - (self.width() - LEFT) / self.pps * 0.1)
        self.update()

    def clip_rect(self, i: int) -> QRectF:
        p = self.project
        r = self.rows()['video']
        a = p.clip_start(i)
        return QRectF(self.x_of(a), r.top(), p.clips[i].duration * self.pps, r.height())

    def text_rect(self, tid: int) -> QRectF:
        t = self.project.text(tid)
        r = self.rows()['text']
        return QRectF(self.x_of(t.start), r.top() + 2, max(4.0, (t.end - t.start) * self.pps), r.height() - 4)

    # painting ------------------------------------------------------------------------------------
    def paintEvent(self, _e: QPaintEvent) -> None:  # noqa: N802
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.Antialiasing)
        p.fillRect(self.rect(), QColor('#0d1326'))
        rows = self.rows()
        small = QFont(self.font())
        small.setPointSizeF(max(7.0, small.pointSizeF() - 1.5))
        p.setFont(small)
        for key, label in (('video', tr('Video')), ('text', tr('Text')), ('music', tr('Music'))):
            r = rows[key]
            p.fillRect(QRectF(LEFT, r.top(), self.width() - LEFT, r.height()), QColor('#10172a'))
            p.setPen(QColor('#8d9bb8'))
            p.drawText(QRectF(6, r.top(), LEFT - 10, r.height()), Qt.AlignmentFlag.AlignVCenter, label)
        if self.project is None:
            return
        proj = self.project
        p.save()
        p.setClipRect(QRectF(LEFT, 0, self.width() - LEFT, self.height()))
        self._paint_ruler(p, rows['ruler'])
        # In / Out range
        if self.mark_in is not None or self.mark_out is not None:
            a = self.mark_in if self.mark_in is not None else 0.0
            b = self.mark_out if self.mark_out is not None else proj.duration
            lo, hi = sorted((a, b))
            p.fillRect(
                QRectF(
                    self.x_of(lo),
                    rows['ruler'].bottom(),
                    (hi - lo) * self.pps,
                    self.height() - rows['ruler'].bottom(),
                ),
                QColor(251, 191, 36, 38),
            )
            for m, txt in ((self.mark_in, 'I'), (self.mark_out, 'O')):
                if m is None:
                    continue
                x = self.x_of(m)
                p.setPen(QPen(QColor('#fbbf24'), 1.5))
                p.drawLine(QPointF(x, rows['ruler'].top()), QPointF(x, self.height()))
                p.setBrush(QColor('#fbbf24'))
                p.drawRect(QRectF(x - (12 if txt == 'O' else 0), 0, 12, 13))
                p.setPen(QColor('#0b1020'))
                p.drawText(
                    QRectF(x - (12 if txt == 'O' else 0), 0, 12, 13), Qt.AlignmentFlag.AlignCenter, txt
                )
        # clips
        for i, c in enumerate(proj.clips):
            r = self.clip_rect(i).adjusted(1, 2, -1, -2)
            p.setPen(QPen(ACCENT, 2) if i == self.sel_clip else QPen(QColor('#4f8cff'), 1))
            p.setBrush(QColor(CLIP_COLORS[i % 2]))
            p.drawRoundedRect(r, 6, 6)
            p.setPen(QColor('#ffffff'))
            label = f'#{i + 1}  {fmt_time(c.start)} – {fmt_time(c.end)}'
            if r.width() > 40:
                p.drawText(
                    r.adjusted(8, 4, -6, -4),
                    Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignTop,
                    p.fontMetrics().elidedText(label, Qt.TextElideMode.ElideRight, int(r.width() - 14)),
                )
                p.setPen(QColor(255, 255, 255, 150))
                p.drawText(
                    r.adjusted(8, 4, -6, -4),
                    Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignBottom,
                    f'{c.duration:.1f} s',
                )
        # texts
        for t in proj.texts:
            r = self.text_rect(t.id)
            p.setPen(QPen(ACCENT, 2) if t.id == self.sel_text else QPen(QColor('#8b5cf6'), 1))
            p.setBrush(QColor('#5b3fb0'))
            p.drawRoundedRect(r, 5, 5)
            p.setPen(QColor('#ffffff'))
            p.drawText(
                r.adjusted(6, 0, -4, 0),
                Qt.AlignmentFlag.AlignVCenter,
                p.fontMetrics().elidedText(
                    t.text.replace('\n', ' ') or '…', Qt.TextElideMode.ElideRight, int(max(0, r.width() - 10))
                ),
            )
        # music
        if proj.music is not None:
            mr = rows['music']
            r = QRectF(self.x_of(0), mr.top() + 2, proj.music_end() * self.pps, mr.height() - 4)
            p.setPen(QPen(ACCENT, 2) if self.music_selected_flag else QPen(QColor('#34d399'), 1))
            p.setBrush(QColor('#1f6b52'))
            p.drawRoundedRect(r, 5, 5)
            p.setPen(QColor('#ffffff'))
            p.drawText(
                r.adjusted(6, 0, -4, 0),
                Qt.AlignmentFlag.AlignVCenter,
                p.fontMetrics().elidedText(
                    f'♪ {proj.music.name}', Qt.TextElideMode.ElideRight, int(max(0, r.width() - 10))
                ),
            )
        # playhead
        x = self.x_of(self.t)
        p.setPen(QPen(QColor('#f87171'), 2))
        p.drawLine(QPointF(x, 0), QPointF(x, self.height()))
        p.setBrush(QColor('#f87171'))
        p.setPen(Qt.PenStyle.NoPen)
        p.drawPolygon(QPolygonF([QPointF(x - 6, 0), QPointF(x + 6, 0), QPointF(x, 9)]))
        p.restore()

    def _paint_ruler(self, p: QPainter, r: QRectF) -> None:
        step = next((s for s in STEPS if s * self.pps >= 70), STEPS[-1])
        t0 = max(0.0, self.t_of(LEFT))
        t1 = self.t_of(self.width())
        k = int(t0 // step)
        p.setPen(QColor('#5d6b88'))
        while k * step <= t1:
            t = k * step
            x = self.x_of(t)
            p.drawLine(QPointF(x, r.bottom() - 7), QPointF(x, r.bottom()))
            p.drawText(
                QRectF(x + 3, r.top(), 80, r.height() - 6),
                Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignVCenter,
                fmt_time(t, 1 if step < 1 else 0),
            )
            half = x + step * self.pps / 2
            p.drawLine(QPointF(half, r.bottom() - 3), QPointF(half, r.bottom()))
            k += 1
        end = self.x_of(self.project.duration)
        p.setPen(QPen(QColor('#2a3a5c'), 1, Qt.PenStyle.DashLine))
        p.drawLine(QPointF(end, r.top()), QPointF(end, self.height()))

    # input ---------------------------------------------------------------------------------------
    def _hit(self, pos: QPointF) -> dict:
        p = self.project
        rows = self.rows()
        if p is None or pos.x() < LEFT:
            return {'kind': 'none'}
        if rows['video'].contains(pos):
            for i in range(len(p.clips)):
                r = self.clip_rect(i)
                if r.left() - 2 <= pos.x() <= r.right() + 2:
                    if abs(pos.x() - r.left()) <= EDGE and i > -1:
                        return {'kind': 'clip_edge', 'index': i, 'side': 'start'}
                    if abs(pos.x() - r.right()) <= EDGE:
                        return {'kind': 'clip_edge', 'index': i, 'side': 'end'}
                    return {'kind': 'clip', 'index': i}
        if rows['text'].contains(pos):
            for t in reversed(p.texts):
                r = self.text_rect(t.id)
                if r.left() - EDGE <= pos.x() <= r.right() + EDGE:
                    if abs(pos.x() - r.left()) <= EDGE:
                        return {'kind': 'text_edge', 'id': t.id, 'side': 'start'}
                    if abs(pos.x() - r.right()) <= EDGE:
                        return {'kind': 'text_edge', 'id': t.id, 'side': 'end'}
                    return {'kind': 'text', 'id': t.id}
        if rows['music'].contains(pos) and p.music is not None and pos.x() <= self.x_of(p.music_end()):
            return {'kind': 'music'}
        return {'kind': 'seek'}

    def mousePressEvent(self, e: QMouseEvent) -> None:  # noqa: N802
        if self.project is None or e.button() != Qt.MouseButton.LeftButton:
            return
        pos = e.position()
        hit = self._hit(pos)
        kind = hit['kind']
        self._drag = {'kind': kind, 'x0': pos.x(), 'started': False, **hit}
        if kind in ('clip', 'clip_edge'):
            i = hit['index']
            self.sel_clip = i
            c = self.project.clips[i]
            self._drag.update(start0=c.start, end0=c.end)
            self.clip_selected.emit(i)
            if kind == 'clip':
                self.seek_requested.emit(max(0.0, min(self.project.duration, self.t_of(pos.x()))))
        elif kind in ('text', 'text_edge'):
            t = self.project.text(hit['id'])
            self.sel_text = t.id
            self._drag.update(start0=t.start, end0=t.end)
            self.text_selected.emit(t.id)
        elif kind == 'music':
            self.music_selected_flag = True
            self.music_selected.emit()
        elif kind == 'seek':
            self._drag['kind'] = 'playhead'
            self.seek_requested.emit(max(0.0, min(self.project.duration, self.t_of(pos.x()))))
        if kind not in ('music',):
            self.music_selected_flag = False
        self.update()

    def mouseMoveEvent(self, e: QMouseEvent) -> None:  # noqa: N802
        pos = e.position()
        if self._drag is None:
            hit = self._hit(pos)['kind']
            self.setCursor(
                Qt.CursorShape.SizeHorCursor
                if hit in ('clip_edge', 'text_edge')
                else Qt.CursorShape.OpenHandCursor
                if hit == 'text'
                else Qt.CursorShape.ArrowCursor
            )
            return
        d = self._drag
        p = self.project
        if p is None:
            return
        dt = (pos.x() - d['x0']) / self.pps
        if d['kind'] == 'playhead':
            self.seek_requested.emit(max(0.0, min(p.duration, self.t_of(pos.x()))))
            return
        if d['kind'] not in ('clip_edge', 'text', 'text_edge'):
            return
        if not d['started']:
            if abs(pos.x() - d['x0']) < 3:
                return
            d['started'] = True
            self.fit = False  # keep the scale while dragging
            self.edit_started.emit(
                {'clip_edge': 'Trim clip', 'text': 'Move text', 'text_edge': 'Text timing'}[d['kind']]
            )
        if d['kind'] == 'clip_edge':
            i = d['index']
            if d['side'] == 'start':
                trim_clip(p, i, d['start0'] + dt, d['end0'])
            else:
                trim_clip(p, i, d['start0'], d['end0'] + dt)
        else:
            t = p.text(d['id'])
            if t is None:
                return
            length = d['end0'] - d['start0']
            if d['kind'] == 'text':
                t.start = max(0.0, d['start0'] + dt)
                t.end = t.start + length
            elif d['side'] == 'start':
                t.start = min(max(0.0, d['start0'] + dt), t.end - 0.1)
            else:
                t.end = max(t.start + 0.1, d['end0'] + dt)
        self.edited.emit()
        self.update()

    def mouseReleaseEvent(self, _e: QMouseEvent) -> None:  # noqa: N802
        was_trim = self._drag is not None and self._drag.get('started') and self._drag['kind'] == 'clip_edge'
        self._drag = None
        if was_trim:
            self.refit()

    def wheelEvent(self, e: QWheelEvent) -> None:  # noqa: N802
        if self.project is None:
            return
        steps = e.angleDelta().y() / 120.0
        if e.modifiers() & Qt.KeyboardModifier.ControlModifier:
            anchor_t = self.t_of(e.position().x())
            self.pps = min(2000.0, max(1e-3, self.pps * (1.25**steps)))
            self.view_start = max(0.0, anchor_t - (e.position().x() - LEFT) / self.pps)
            self.fit = False
        else:
            self.view_start = max(0.0, self.view_start - steps * 60 / self.pps)
            self.fit = False
        self.update()
        e.accept()
