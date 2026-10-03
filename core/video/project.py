"""Video project: kept clips (in order), mute / volume, texts, music — and timeline edits.

Times: a :class:`Clip` uses *source* seconds; texts and the playhead use *timeline*
seconds (the clips played one after the other).
"""

from __future__ import annotations

import copy
import itertools
from dataclasses import dataclass, field
from pathlib import Path

from core.system import DEFAULT_FONT_FAMILY
from core.video.ffmpeg import MediaInfo

MIN_CLIP = 0.1  # s
_ids = itertools.count(1)


@dataclass
class Clip:
    start: float
    end: float

    @property
    def duration(self) -> float:
        return max(0.0, self.end - self.start)


@dataclass
class TextOverlay:
    id: int = field(default_factory=lambda: next(_ids))
    text: str = ''
    font_family: str = DEFAULT_FONT_FAMILY
    font_size: float = 64.0  # px at the source resolution
    color: str = '#ffffff'
    bold: bool = True
    italic: bool = False
    outline_width: float = 3.0
    outline_color: str = '#000000'
    opacity: float = 1.0
    x: float = 0.5  # centre, fraction of the frame width
    y: float = 0.85  # centre, fraction of the frame height
    start: float = 0.0  # timeline s
    end: float = 3.0

    def active_at(self, t: float) -> bool:
        return self.start <= t < self.end


@dataclass
class MusicTrack:
    path: str
    duration: float  # length of the music file, s
    offset: float = 0.0  # the music starts from this second of the file
    volume: float = 1.0  # 0–2
    fade_out: bool = True
    fade_seconds: float = 2.0

    @property
    def name(self) -> str:
        return Path(self.path).name


@dataclass
class VideoProject:
    source: str
    info: MediaInfo
    clips: list[Clip] = field(default_factory=list)
    mute: bool = False
    volume: float = 1.0  # original sound, 0–2
    texts: list[TextOverlay] = field(default_factory=list)
    music: MusicTrack | None = None

    @classmethod
    def new(cls, source: Path, info: MediaInfo) -> VideoProject:
        return cls(str(source), info, [Clip(0.0, info.duration)])

    def clone(self) -> VideoProject:
        return copy.deepcopy(self)

    # timeline <-> source
    @property
    def duration(self) -> float:
        return sum(c.duration for c in self.clips)

    def clip_start(self, index: int) -> float:
        return sum(c.duration for c in self.clips[:index])

    def locate(self, t: float) -> tuple[int, float]:
        """``(clip index, source time)`` of timeline time ``t`` (clamped to the timeline)."""
        if not self.clips:
            return -1, 0.0
        t = max(0.0, t)
        acc = 0.0
        for i, c in enumerate(self.clips):
            if t < acc + c.duration or i == len(self.clips) - 1:
                return i, min(c.end, c.start + (t - acc))
            acc += c.duration
        return len(self.clips) - 1, self.clips[-1].end

    def timeline_time(self, index: int, source_t: float) -> float:
        c = self.clips[index]
        return self.clip_start(index) + min(max(source_t - c.start, 0.0), c.duration)

    def text(self, text_id: int) -> TextOverlay | None:
        return next((t for t in self.texts if t.id == text_id), None)

    def music_end(self) -> float:
        """Timeline second where the music stops (shorter music or end of the video)."""
        if self.music is None:
            return 0.0
        return max(0.0, min(self.duration, self.music.duration - self.music.offset))


# Timeline edits (in place; the caller records the undo step first) --------------------------


def split_at(p: VideoProject, t: float) -> bool:
    """Split the clip under timeline time ``t`` in two."""
    i, src = p.locate(t)
    if i < 0:
        return False
    c = p.clips[i]
    if src - c.start < MIN_CLIP or c.end - src < MIN_CLIP:
        return False
    p.clips[i : i + 1] = [Clip(c.start, src), Clip(src, c.end)]
    return True


def delete_clip(p: VideoProject, index: int) -> bool:
    if not 0 <= index < len(p.clips) or len(p.clips) == 1:
        return False
    del p.clips[index]
    return True


def move_clip(p: VideoProject, index: int, delta: int) -> bool:
    new = index + delta
    if not 0 <= index < len(p.clips) or not 0 <= new < len(p.clips):
        return False
    p.clips.insert(new, p.clips.pop(index))
    return True


def trim_clip(p: VideoProject, index: int, start: float, end: float) -> None:
    dur = p.info.duration
    start = min(max(0.0, start), dur - MIN_CLIP)
    end = min(max(start + MIN_CLIP, end), dur)
    p.clips[index] = Clip(start, end)


def add_clip(p: VideoProject, start: float, end: float) -> bool:
    dur = p.info.duration
    start, end = max(0.0, min(start, end)), min(dur, max(start, end))
    if end - start < MIN_CLIP:
        return False
    p.clips.append(Clip(start, end))
    return True


def _pieces(p: VideoProject, t0: float, t1: float, inside: bool) -> list[Clip]:
    """Parts of the clips inside (or outside) the timeline range ``[t0, t1]``."""
    out: list[Clip] = []
    acc = 0.0
    for c in p.clips:
        a, b = acc, acc + c.duration  # this clip on the timeline
        acc = b
        if inside:
            lo, hi = max(a, t0), min(b, t1)
            if hi - lo >= MIN_CLIP / 2:
                out.append(Clip(c.start + lo - a, c.start + hi - a))
        else:
            if t0 - a >= MIN_CLIP / 2:
                out.append(Clip(c.start, c.start + min(b, t0) - a))
            if b - t1 >= MIN_CLIP / 2:
                out.append(Clip(c.start + max(a, t1) - a, c.end))
    return out


def keep_range(p: VideoProject, t0: float, t1: float) -> bool:
    """Keep only the timeline range ``[t0, t1]`` (marked In / Out)."""
    t0, t1 = sorted((t0, t1))
    clips = _pieces(p, t0, t1, inside=True)
    if not clips:
        return False
    p.clips = clips
    return True


def delete_range(p: VideoProject, t0: float, t1: float) -> bool:
    """Remove the timeline range ``[t0, t1]`` (e.g. a part in the middle)."""
    t0, t1 = sorted((t0, t1))
    clips = _pieces(p, t0, t1, inside=False)
    if not clips or t1 - t0 < MIN_CLIP / 2:
        return False
    p.clips = clips
    return True


def fmt_time(t: float, decimals: int = 1) -> str:
    """``m:ss.s`` (or ``h:mm:ss.s``)."""
    t = max(0.0, t)
    h, rem = divmod(t, 3600)
    m, s = divmod(rem, 60)
    sec = f'{s:0{3 + decimals if decimals else 2}.{decimals}f}'
    return f'{int(h)}:{int(m):02d}:{sec}' if h else f'{int(m)}:{sec}'
