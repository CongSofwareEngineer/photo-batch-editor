"""Video preview: plays the edited timeline (clips in order), the music, and draws the texts.

Frames come from a ``QVideoSink`` and are painted by this widget, so texts can be drawn and
dragged over the picture. Playback follows the timeline: at the end of a clip the player
jumps to the start of the next one. Preview sound is limited to 100 % volume (the export
applies the real volume, up to 200 %).
"""

from __future__ import annotations

import logging

from PySide6.QtCore import QPointF, QRectF, Qt, QUrl, Signal
from PySide6.QtGui import QColor, QImage, QMouseEvent, QPainter, QPaintEvent, QPen
from PySide6.QtMultimedia import QAudioOutput, QMediaPlayer, QVideoFrame, QVideoSink
from PySide6.QtWidgets import QWidget

from core.i18n import tr
from core.video.project import VideoProject
from ui.video.text_render import overlay_rect, render_overlay

log = logging.getLogger(__name__)

END_EPS = 0.04  # s before the end of a clip where playback jumps to the next one
MUSIC_DRIFT_MS = 300


class VideoPreview(QWidget):
    position_changed = Signal(float)  # timeline seconds
    playing_changed = Signal(bool)
    text_selected = Signal(int)  # text id, 0 = none
    text_drag_started = Signal()  # the view records an undo step
    text_moved = Signal()
    media_error = Signal(str)

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName('VideoPreview')
        self.setMinimumSize(360, 220)
        self.setMouseTracking(True)
        self.player = QMediaPlayer(self)
        self.audio = QAudioOutput(self)
        self.player.setAudioOutput(self.audio)
        self.sink = QVideoSink(self)
        self.player.setVideoSink(self.sink)
        self.music = QMediaPlayer(self)
        self.music_audio = QAudioOutput(self)
        self.music.setAudioOutput(self.music_audio)
        self.project: VideoProject | None = None
        self.frame = QImage()
        self.t = 0.0
        self.playing = False
        self.selected_text = 0
        self._clip = 0
        self._source = ''
        self._music_source = ''
        self._drag: tuple[int, QPointF, float, float] | None = None
        self._dragging = False
        self._primed = False
        self.sink.videoFrameChanged.connect(self._frame)
        self.player.positionChanged.connect(self._source_position)
        self.player.mediaStatusChanged.connect(self._media_status)
        self.player.errorOccurred.connect(lambda _e, msg: self.media_error.emit(msg))

    # project ---------------------------------------------------------------------------------
    def set_project(self, p: VideoProject | None) -> None:
        self.project = p
        if p is None:
            self.stop_media()
            return
        if p.source != self._source:
            self._source = p.source
            self.frame = QImage()
            self._primed = False
            self.player.setSource(QUrl.fromLocalFile(p.source))
        self.update_music_source()
        self.apply_audio()
        self.seek(min(self.t, p.duration))

    def stop_media(self) -> None:
        self.pause()
        self.player.setSource(QUrl())
        self.music.setSource(QUrl())
        self._source = self._music_source = ''
        self.frame = QImage()
        self.update()

    def update_music_source(self) -> None:
        m = self.project.music if self.project else None
        path = m.path if m else ''
        if path != self._music_source:
            self._music_source = path
            self.music.setSource(QUrl.fromLocalFile(path) if path else QUrl())
        self._sync_music(force=True)

    def apply_audio(self) -> None:
        p = self.project
        if p is None:
            return
        self.audio.setVolume(0.0 if p.mute or not p.info.has_audio else min(1.0, p.volume))
        self._sync_music()

    def _media_status(self, status: QMediaPlayer.MediaStatus) -> None:
        # once per file: pausing / seeking can report LoadedMedia again
        if status == QMediaPlayer.MediaStatus.LoadedMedia and not self.playing and not self._primed:
            self._primed = True
            self.player.pause()  # decode and show the first frame
            self.seek(self.t)

    def _frame(self, frame: QVideoFrame) -> None:
        if frame.isValid():
            img = frame.toImage()
            if not img.isNull():
                self.frame = img
                self.update()

    # transport -------------------------------------------------------------------------------
    def play(self) -> None:
        p = self.project
        if p is None or not p.clips:
            return
        if self.t >= p.duration - END_EPS:
            self.seek(0.0)
        self.playing = True
        self._clip, _src = p.locate(self.t)
        self.player.play()
        self._sync_music(force=True)
        self.playing_changed.emit(True)

    def pause(self) -> None:
        was = self.playing
        self.playing = False
        if self.player.playbackState() == QMediaPlayer.PlaybackState.PlayingState:
            self.player.pause()
        self.music.pause()
        if was:
            self.playing_changed.emit(False)

    def toggle(self) -> None:
        if self.playing:
            self.pause()
        else:
            self.play()

    def seek(self, t: float) -> None:
        p = self.project
        if p is None or not p.clips:
            return
        self.t = min(max(0.0, t), p.duration)
        self._clip, src = p.locate(self.t)
        self.player.setPosition(int(round(src * 1000)))
        self._sync_music(force=True)
        self.position_changed.emit(self.t)
        self.update()

    def _source_position(self, ms: int) -> None:
        p = self.project
        if p is None or not p.clips or not self.playing:
            return
        if not 0 <= self._clip < len(p.clips):
            self._clip = 0
        c = p.clips[self._clip]
        src = ms / 1000.0
        if src < c.start - 0.3 or src > c.end + 0.6:
            return  # a position from before the last jump
        if src >= c.end - END_EPS:
            if self._clip + 1 < len(p.clips):
                self._clip += 1
                self.player.setPosition(int(round(p.clips[self._clip].start * 1000)))
                self.t = p.clip_start(self._clip)
            else:
                self.pause()
                self.t = p.duration
                self.player.setPosition(int(round(c.end * 1000)))
            self.position_changed.emit(self.t)
            self._sync_music(force=True)
            self.update()
            return
        self.t = p.timeline_time(self._clip, src)
        self.position_changed.emit(self.t)
        self._sync_music()
        self.update()

    def _sync_music(self, force: bool = False) -> None:
        p = self.project
        m = p.music if p else None
        if p is None or m is None:
            if self.music.playbackState() != QMediaPlayer.PlaybackState.StoppedState:
                self.music.stop()
            return
        end = p.music_end()
        if self.t >= end:
            self.music.pause()
            return
        gain = 1.0
        if m.fade_out and m.fade_seconds > 0 and self.t > end - m.fade_seconds:
            gain = max(0.0, (end - self.t) / m.fade_seconds)
        self.music_audio.setVolume(min(1.0, m.volume) * gain)
        want = int(round((m.offset + self.t) * 1000))
        if force or abs(self.music.position() - want) > MUSIC_DRIFT_MS:
            self.music.setPosition(want)
        if self.playing and self.music.playbackState() != QMediaPlayer.PlaybackState.PlayingState:
            self.music.play()
        elif not self.playing and self.music.playbackState() == QMediaPlayer.PlaybackState.PlayingState:
            self.music.pause()

    # painting --------------------------------------------------------------------------------
    def frame_rect(self) -> QRectF:
        p = self.project
        w, h = (p.info.width, p.info.height) if p and p.info.width else (16, 9)
        avail = QRectF(self.rect()).adjusted(8, 8, -8, -8)
        k = min(avail.width() / w, avail.height() / h)
        return QRectF(avail.center().x() - w * k / 2, avail.center().y() - h * k / 2, w * k, h * k)

    def _scale(self) -> float:
        p = self.project
        return self.frame_rect().height() / max(1, p.info.height) if p else 1.0

    def paintEvent(self, _e: QPaintEvent) -> None:  # noqa: N802
        painter = QPainter(self)
        painter.fillRect(self.rect(), QColor('#080c18'))
        p = self.project
        if p is None:
            painter.setPen(QColor('#5d6b88'))
            painter.drawText(
                self.rect(), Qt.AlignmentFlag.AlignCenter, tr('Open a video (Ctrl+O) or drop a file here')
            )
            return
        fr = self.frame_rect()
        painter.fillRect(fr, QColor('#000000'))
        if not self.frame.isNull():
            painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform)
            painter.drawImage(fr, self.frame)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        scale = self._scale()
        painter.save()
        painter.setClipRect(fr)
        for t in p.texts:
            if t.active_at(self.t) or t.id == self.selected_text:
                img = render_overlay(t, scale)
                if img.isNull():
                    continue
                r = overlay_rect(t, fr, scale)
                painter.setOpacity(1.0 if t.active_at(self.t) else 0.35)
                painter.drawImage(r.topLeft(), img)
                painter.setOpacity(1.0)
                if t.id == self.selected_text:
                    painter.setPen(QPen(QColor('#22d3ee'), 1.4, Qt.PenStyle.DashLine))
                    painter.setBrush(Qt.BrushStyle.NoBrush)
                    painter.drawRect(r.adjusted(-3, -3, 3, 3))
        painter.restore()

    # dragging texts ----------------------------------------------------------------------------
    def text_at(self, pos: QPointF) -> int:
        p = self.project
        if p is None:
            return 0
        fr, scale = self.frame_rect(), self._scale()
        for t in reversed(p.texts):
            if (t.active_at(self.t) or t.id == self.selected_text) and overlay_rect(t, fr, scale).adjusted(
                -4, -4, 4, 4
            ).contains(pos):
                return t.id
        return 0

    def mousePressEvent(self, e: QMouseEvent) -> None:  # noqa: N802
        if self.project is None or e.button() != Qt.MouseButton.LeftButton:
            return
        tid = self.text_at(e.position())
        self.selected_text = tid
        self.text_selected.emit(tid)
        t = self.project.text(tid)
        self._drag = (tid, e.position(), t.x, t.y) if t else None
        self._dragging = False
        self.update()

    def mouseMoveEvent(self, e: QMouseEvent) -> None:  # noqa: N802
        if self._drag is None:
            self.setCursor(
                Qt.CursorShape.SizeAllCursor if self.text_at(e.position()) else Qt.CursorShape.ArrowCursor
            )
            return
        tid, start, x0, y0 = self._drag
        d = e.position() - start
        if not self._dragging:
            if abs(d.x()) + abs(d.y()) < 3:
                return
            self._dragging = True
            self.text_drag_started.emit()
        t = self.project.text(tid) if self.project else None
        if t is None:
            return
        fr = self.frame_rect()
        t.x = min(1.0, max(0.0, x0 + d.x() / fr.width()))
        t.y = min(1.0, max(0.0, y0 + d.y() / fr.height()))
        self.text_moved.emit()
        self.update()

    def mouseReleaseEvent(self, _e: QMouseEvent) -> None:  # noqa: N802
        self._drag = None
        self._dragging = False
