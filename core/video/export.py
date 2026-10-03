"""Building and running the FFmpeg command that exports a :class:`VideoProject`.

Filter graph (``-filter_complex``):

* every clip: ``trim`` / ``atrim`` + ``setpts``, then ``concat`` in the clip order;
* ``scale`` to 1080p / 720p (short side, only down), ``setsar=1``;
* text: one transparent PNG per text (drawn by the app, so it looks exactly like the
  preview and Vietnamese accents are right), ``overlay`` with ``enable='between(t,a,b)'``;
* audio: original × volume (or none when muted / no audio) mixed with the music
  (``atrim`` to the video length, volume, optional ``afade`` out).

The result is written to ``<name>.tmp.<ext>`` and renamed when FFmpeg succeeds, so a failed
or cancelled export never leaves a broken file; the source is never overwritten.
"""

from __future__ import annotations

import os
import threading
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

from core.video.ffmpeg import run_ffmpeg
from core.video.project import VideoProject

EDITOR_FOLDER = 'editor'
FORMATS: dict[str, str] = {'mp4': '.mp4', 'mov': '.mov'}
RESOLUTIONS: dict[str, int | None] = {'original': None, '1080p': 1080, '720p': 720}
SAME_AS_SOURCE = 'The output file cannot be the original video'


@dataclass
class OverlayImage:
    path: str  # transparent PNG at the output resolution
    x: int  # top-left in output px
    y: int
    start: float  # timeline s
    end: float


def _even(v: float) -> int:
    return max(2, int(round(v / 2)) * 2)


def output_size(width: int, height: int, resolution: str) -> tuple[int, int]:
    """Output frame size: the short side becomes 1080 / 720 (never enlarged); even numbers."""
    target = RESOLUTIONS.get(resolution)
    short = min(width, height)
    if not target or short <= target:
        return _even(width), _even(height)
    k = target / short
    return _even(width * k), _even(height * k)


def default_output_path(source: Path, fmt: str) -> Path:
    source = Path(source)
    return source.parent / EDITOR_FOLDER / f'{source.stem}{FORMATS.get(fmt, ".mp4")}'


def _f(v: float) -> str:
    return f'{max(0.0, v):.3f}'


def build_args(
    p: VideoProject,
    overlays: list[OverlayImage],
    out_path: Path,
    fmt: str = 'mp4',
    resolution: str = 'original',
) -> list[str]:
    """FFmpeg arguments (without the executable / progress options)."""
    if not p.clips:
        raise ValueError('Nothing to export: the timeline is empty')
    duration = p.duration
    use_orig_audio = p.info.has_audio and not p.mute and p.volume > 0
    args: list[str] = ['-i', p.source]
    for ov in overlays:
        args += ['-i', ov.path]
    music_index = None
    if p.music is not None and p.music_end() > 0.05:
        music_index = 1 + len(overlays)
        if p.music.offset > 0:
            args += ['-ss', _f(p.music.offset)]
        args += ['-i', p.music.path]

    f: list[str] = []
    n = len(p.clips)
    for k, c in enumerate(p.clips):
        f.append(f'[0:v]trim=start={_f(c.start)}:end={_f(c.end)},setpts=PTS-STARTPTS[v{k}]')
        if use_orig_audio:
            f.append(f'[0:a]atrim=start={_f(c.start)}:end={_f(c.end)},asetpts=PTS-STARTPTS[a{k}]')
    if n > 1:
        ins = ''.join(f'[v{k}][a{k}]' if use_orig_audio else f'[v{k}]' for k in range(n))
        f.append(
            f'{ins}concat=n={n}:v=1:a={1 if use_orig_audio else 0}[vc]' + ('[ac]' if use_orig_audio else '')
        )
    else:
        f.append('[v0]null[vc]')
        if use_orig_audio:
            f.append('[a0]anull[ac]')

    w, h = output_size(p.info.width, p.info.height, resolution)
    if (w, h) != (p.info.width, p.info.height):
        f.append(f'[vc]scale={w}:{h}:flags=lanczos,setsar=1[vs]')
    else:
        f.append('[vc]setsar=1[vs]')
    cur = 'vs'
    for i, ov in enumerate(overlays):
        nxt = f'vo{i}'
        f.append(
            f'[{cur}][{i + 1}:v]overlay=x={ov.x}:y={ov.y}:'
            f"enable='between(t,{_f(ov.start)},{_f(ov.end)})'[{nxt}]"
        )
        cur = nxt
    f.append(f'[{cur}]format=yuv420p[vout]')

    audio_label = None
    if use_orig_audio:
        f.append(f'[ac]volume={p.volume:.3f}[aorig]')
        audio_label = '[aorig]'
    if music_index is not None:
        m = p.music
        assert m is not None
        end = p.music_end()
        chain = f'[{music_index}:a]atrim=0:{_f(end)},asetpts=PTS-STARTPTS,volume={m.volume:.3f}'
        if m.fade_out and m.fade_seconds > 0:
            fade = min(m.fade_seconds, end)
            chain += f',afade=t=out:st={_f(end - fade)}:d={_f(fade)}'
        f.append(chain + '[amus]')
        if audio_label:
            f.append(
                f'{audio_label}[amus]amix=inputs=2:duration=first:dropout_transition=0:normalize=0[aout]'
            )
            audio_label = '[aout]'
        else:
            audio_label = '[amus]'

    args += ['-filter_complex', ';'.join(f), '-map', '[vout]']
    if audio_label:
        args += ['-map', audio_label, '-c:a', 'aac', '-b:a', '192k']
    else:
        args += ['-an']
    args += ['-c:v', 'libx264', '-preset', 'fast', '-crf', '20', '-t', _f(duration)]
    if p.info.fps > 0:  # constant frame rate (concat can leave small timestamp gaps)
        args += ['-fps_mode', 'cfr', '-r', f'{min(p.info.fps, 120.0):.3f}']
    if fmt == 'mp4':
        args += ['-movflags', '+faststart']
    args.append(str(out_path))
    return args


def tmp_output(out_path: Path) -> Path:
    return out_path.with_name(f'{out_path.stem}.tmp{out_path.suffix}')


def export_video(
    p: VideoProject,
    overlays: list[OverlayImage],
    out_path: Path,
    fmt: str = 'mp4',
    resolution: str = 'original',
    on_progress: Callable[[float], None] | None = None,
    cancel_event: threading.Event | None = None,
) -> Path:
    out_path = Path(out_path)
    if out_path.resolve() == Path(p.source).resolve():
        raise ValueError(SAME_AS_SOURCE)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    tmp = tmp_output(out_path)
    try:
        run_ffmpeg(build_args(p, overlays, tmp, fmt, resolution), p.duration, on_progress, cancel_event)
        os.replace(tmp, out_path)
    finally:
        if tmp.exists():
            try:
                tmp.unlink()
            except OSError:
                pass
    return out_path
