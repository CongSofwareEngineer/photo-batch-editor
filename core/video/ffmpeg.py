"""FFmpeg: finding the executable, reading media information, running with progress / cancel.

Search order for ``ffmpeg.exe``:

1. ``PBE_FFMPEG`` environment variable (full path),
2. ``ffmpeg/ffmpeg.exe`` bundled with the app (the .exe build copies it there),
3. the binary of the ``imageio-ffmpeg`` pip package (running from source),
4. ``ffmpeg`` on the ``PATH``.
"""

from __future__ import annotations

import logging
import os
import re
import shutil
import subprocess
import sys
import threading
from collections import deque
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

from core.paths import app_root

log = logging.getLogger(__name__)

FFMPEG_NOT_FOUND = (
    'FFmpeg was not found. It should be in the "ffmpeg" folder next to the app; '
    'reinstall the app or put ffmpeg.exe on the PATH.'
)
EXE = 'ffmpeg.exe' if sys.platform == 'win32' else 'ffmpeg'


class FFmpegError(Exception):
    """FFmpeg failed (the message holds the end of its log)."""


class FFmpegNotFound(FFmpegError):
    pass


class Cancelled(Exception):
    pass


_cached: Path | None = None


def _candidates() -> list[Path]:
    out: list[Path] = []
    env = os.environ.get('PBE_FFMPEG')
    if env:
        out.append(Path(env))
    out.append(app_root() / 'ffmpeg' / EXE)
    if getattr(sys, 'frozen', False):
        out.append(Path(sys.executable).resolve().parent / 'ffmpeg' / EXE)
    try:
        import imageio_ffmpeg  # noqa: PLC0415

        out.append(Path(imageio_ffmpeg.get_ffmpeg_exe()))
    except Exception:  # noqa: BLE001 - package missing or its binary missing
        pass
    found = shutil.which('ffmpeg')
    if found:
        out.append(Path(found))
    return out


def find_ffmpeg(refresh: bool = False) -> Path:
    global _cached
    if _cached is not None and not refresh and _cached.is_file():
        return _cached
    for c in _candidates():
        if c.is_file():
            _cached = c
            return c
    raise FFmpegNotFound(FFMPEG_NOT_FOUND)


def popen_kwargs() -> dict:
    """No console window flashing on Windows (GUI app)."""
    if sys.platform == 'win32':
        return {'creationflags': getattr(subprocess, 'CREATE_NO_WINDOW', 0x08000000)}
    return {}


# Media information ------------------------------------------------------------------------


@dataclass
class MediaInfo:
    duration: float  # seconds
    width: int = 0  # display size (rotation applied)
    height: int = 0
    fps: float = 0.0
    has_video: bool = False
    has_audio: bool = False
    rotation: int = 0
    video_codec: str = ''
    audio_codec: str = ''


_DURATION = re.compile(r'Duration:\s*(\d+):(\d+):(\d+(?:\.\d+)?)')
_VIDEO = re.compile(r'Stream #\d+:\d+.*?: Video: (\w+).*?[, ](\d{2,5})x(\d{2,5})')
_FPS = re.compile(r'(\d+(?:\.\d+)?) fps')
_TBR = re.compile(r'(\d+(?:\.\d+)?) tbr')
_AUDIO = re.compile(r'Stream #\d+:\d+.*?: Audio: (\w+)')
_ROTATION = re.compile(r'rotation of (-?\d+(?:\.\d+)?) degrees|rotate\s*:\s*(-?\d+)')


def parse_probe(text: str) -> MediaInfo:
    """Media information from the log of ``ffmpeg -i <file>``."""
    m = _DURATION.search(text)
    if not m:
        raise FFmpegError('Cannot read the duration of this file (is it a video or audio file?)')
    duration = int(m.group(1)) * 3600 + int(m.group(2)) * 60 + float(m.group(3))
    info = MediaInfo(duration=duration)
    for line in text.splitlines():
        if 'Video:' in line and not info.has_video and 'attached pic' not in line:
            v = _VIDEO.search(line)
            if v:
                info.has_video = True
                info.video_codec = v.group(1)
                info.width, info.height = int(v.group(2)), int(v.group(3))
                f = _FPS.search(line) or _TBR.search(line)
                info.fps = float(f.group(1)) if f else 0.0
        elif 'Audio:' in line and not info.has_audio:
            a = _AUDIO.search(line)
            if a:
                info.has_audio = True
                info.audio_codec = a.group(1)
    r = _ROTATION.search(text)
    if r:
        info.rotation = int(round(float(r.group(1) or r.group(2)))) % 360
        if info.rotation in (90, 270):  # FFmpeg auto-rotates: the output is turned
            info.width, info.height = info.height, info.width
    return info


def probe(path: Path) -> MediaInfo:
    exe = find_ffmpeg()
    try:
        res = subprocess.run(
            [str(exe), '-hide_banner', '-i', str(path)],
            capture_output=True,  # noqa: S603
            timeout=60,
            **popen_kwargs(),
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise FFmpegError(f'Cannot run FFmpeg: {exc}') from exc
    text = res.stderr.decode('utf-8', errors='replace')
    return parse_probe(text)


# Running ---------------------------------------------------------------------------------

_OUT_TIME = re.compile(r'^out_time_(?:us|ms)=(\d+)')


def run_ffmpeg(
    args: list[str],
    duration: float,
    on_progress: Callable[[float], None] | None = None,
    cancel_event: threading.Event | None = None,
) -> None:
    """Run ``ffmpeg <args>``; ``on_progress(fraction)`` from FFmpeg's ``-progress`` output.

    Raises :class:`Cancelled` when ``cancel_event`` is set (the process is stopped) and
    :class:`FFmpegError` with the end of the log when FFmpeg fails.
    """
    exe = find_ffmpeg()
    cmd = [str(exe), '-hide_banner', '-nostdin', '-y', *args, '-progress', 'pipe:1', '-nostats']
    log.info('FFmpeg: %s', subprocess.list2cmdline(cmd))
    try:
        proc = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,  # noqa: S603
            stdin=subprocess.DEVNULL,
            **popen_kwargs(),
        )
    except OSError as exc:
        raise FFmpegError(f'Cannot run FFmpeg: {exc}') from exc
    tail: deque[str] = deque(maxlen=25)

    def read_err() -> None:
        assert proc.stderr is not None
        for raw in proc.stderr:
            tail.append(raw.decode('utf-8', errors='replace').rstrip())

    err_thread = threading.Thread(target=read_err, daemon=True)
    err_thread.start()
    cancelled = False
    assert proc.stdout is not None
    watcher = None
    if cancel_event is not None:

        def watch() -> None:
            nonlocal cancelled
            while proc.poll() is None:
                if cancel_event.wait(0.2):
                    cancelled = True
                    try:
                        proc.terminate()
                    except OSError:
                        pass
                    return

        watcher = threading.Thread(target=watch, daemon=True)
        watcher.start()
    for raw in proc.stdout:
        line = raw.decode('utf-8', errors='replace').strip()
        m = _OUT_TIME.match(line)
        if m and on_progress is not None and duration > 0:
            on_progress(min(1.0, int(m.group(1)) / 1e6 / duration))
    rc = proc.wait()
    err_thread.join(2)
    if cancelled or (cancel_event is not None and cancel_event.is_set()):
        raise Cancelled()
    if rc != 0:
        lines = [t for t in tail if t.strip()]
        raise FFmpegError('\n'.join(lines[-8:]) or f'FFmpeg exited with code {rc}')
    if on_progress is not None:
        on_progress(1.0)
