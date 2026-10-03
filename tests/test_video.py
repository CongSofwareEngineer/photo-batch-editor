"""Video editor: FFmpeg probe parsing, timeline edits, export command and real exports."""

import subprocess
import threading
from pathlib import Path

import pytest

from core.video.export import build_args, default_output_path, export_video, output_size
from core.video.ffmpeg import (
    FFMPEG_NOT_FOUND,
    Cancelled,
    FFmpegNotFound,
    MediaInfo,
    find_ffmpeg,
    parse_probe,
    popen_kwargs,
    probe,
)
from core.video.project import (
    Clip,
    MusicTrack,
    TextOverlay,
    VideoProject,
    add_clip,
    delete_clip,
    delete_range,
    fmt_time,
    keep_range,
    move_clip,
    split_at,
    trim_clip,
)
from tests.conftest import pump

PROBE_TEXT = """
Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'C:\\v\\phone.mp4':
  Duration: 00:01:02.50, start: 0.000000, bitrate: 17000 kb/s
  Stream #0:0[0x1](eng): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709), 1920x1080, 16000 kb/s, 29.97 fps, 29.97 tbr, 90k tbn (default)
      Side data:
        displaymatrix: rotation of -90.00 degrees
  Stream #0:1[0x2](eng): Audio: aac (LC) (mp4a / 0x6134706D), 48000 Hz, stereo, fltp, 256 kb/s (default)
"""


def info(duration: float = 10.0, audio: bool = True) -> MediaInfo:
    return MediaInfo(duration=duration, width=1920, height=1080, fps=30.0, has_video=True, has_audio=audio)


def project(duration: float = 10.0, audio: bool = True) -> VideoProject:
    return VideoProject.new(Path('C:/v/in.mp4'), info(duration, audio))


# Probe ------------------------------------------------------------------------------------------


def test_parse_probe_rotated_phone_video():
    i = parse_probe(PROBE_TEXT)
    assert i.duration == pytest.approx(62.5)
    assert (i.width, i.height) == (1080, 1920)  # rotation applied
    assert i.fps == pytest.approx(29.97) and i.has_audio and i.video_codec == 'h264'


def test_parse_probe_audio_only_and_errors():
    i = parse_probe(
        '  Duration: 00:00:20.04, start: 0.025057, bitrate: 128 kb/s\n'
        '  Stream #0:0: Audio: mp3 (mp3float), 44100 Hz, mono, fltp, 128 kb/s\n'
    )
    assert i.has_audio and not i.has_video and i.duration == pytest.approx(20.04)
    with pytest.raises(Exception, match='duration'):
        parse_probe('garbage')


def test_find_ffmpeg_reports_clearly(monkeypatch):
    import core.video.ffmpeg as ff

    monkeypatch.setattr(ff, '_cached', None)
    monkeypatch.setattr(ff, '_candidates', lambda: [Path('Z:/nope/ffmpeg.exe')])
    with pytest.raises(FFmpegNotFound, match='FFmpeg was not found'):
        find_ffmpeg()
    assert 'ffmpeg' in FFMPEG_NOT_FOUND


def test_find_ffmpeg_env_override(monkeypatch, tmp_path):
    import core.video.ffmpeg as ff

    fake = tmp_path / 'ffmpeg.exe'
    fake.write_bytes(b'')
    monkeypatch.setenv('PBE_FFMPEG', str(fake))
    monkeypatch.setattr(ff, '_cached', None)
    assert find_ffmpeg() == fake
    monkeypatch.setattr(ff, '_cached', None)


# Timeline edits ---------------------------------------------------------------------------------


def test_split_locate_and_delete():
    p = project(10)
    assert split_at(p, 4.0) and split_at(p, 7.0)
    assert [(c.start, c.end) for c in p.clips] == [(0, 4), (4, 7), (7, 10)]
    assert not split_at(p, 4.02)  # too close to an edge
    assert p.locate(5.0) == (1, 5.0)
    assert delete_clip(p, 1)  # the middle part is gone
    assert p.duration == pytest.approx(7.0)
    assert p.locate(5.0) == (1, 8.0)
    assert p.timeline_time(1, 8.0) == pytest.approx(5.0)
    assert not delete_clip(project(), 0)  # never the last clip


def test_reorder_trim_add():
    p = project(10)
    split_at(p, 3.0)
    assert move_clip(p, 1, -1)
    assert [(c.start, c.end) for c in p.clips] == [(3, 10), (0, 3)]
    trim_clip(p, 0, 4.0, 20.0)
    assert (p.clips[0].start, p.clips[0].end) == (4.0, 10.0)
    trim_clip(p, 0, 9.99, 9.0)
    assert p.clips[0].duration >= 0.1 - 1e-9
    assert add_clip(p, 1.0, 2.5)
    assert (p.clips[-1].start, p.clips[-1].end) == (1.0, 2.5)


def test_keep_and_delete_range_across_clips():
    p = project(10)
    split_at(p, 5.0)
    move_clip(p, 1, -1)  # timeline: source 5–10, then 0–5
    assert delete_range(p, 3.0, 7.0)  # timeline 3–7 = source 8–10 + 0–2
    assert [(c.start, c.end) for c in p.clips] == [(5, 8), (2, 5)]
    q = project(10)
    assert keep_range(q, 2.0, 6.5)
    assert [(c.start, c.end) for c in q.clips] == [(2.0, 6.5)]
    assert not delete_range(project(10), 0, 10)


def test_music_end_and_fmt_time():
    p = project(10)
    p.music = MusicTrack('m.mp3', duration=8.0, offset=2.0)
    assert p.music_end() == pytest.approx(6.0)
    p.music.offset = 0.0
    p.music.duration = 60.0
    assert p.music_end() == pytest.approx(10.0)  # cut at the end of the video
    assert fmt_time(65.25) == '1:05.2' or fmt_time(65.25) == '1:05.3'
    assert fmt_time(3725, 0) == '1:02:05'


# Export command ---------------------------------------------------------------------------------


def test_output_size():
    assert output_size(1920, 1080, 'original') == (1920, 1080)
    assert output_size(1920, 1080, '720p') == (1280, 720)
    assert output_size(1080, 1920, '720p') == (720, 1280)  # portrait: short side
    assert output_size(1280, 720, '1080p') == (1280, 720)  # never enlarged
    w, h = output_size(1001, 563, 'original')  # even sizes for H.264
    assert w % 2 == 0 and h % 2 == 0 and abs(w - 1001) <= 1 and abs(h - 563) <= 1


def test_default_output_path_in_editor_folder():
    assert default_output_path(Path('D:/clips/a b.mov'), 'mp4') == Path('D:/clips/editor/a b.mp4')


def test_build_args_segments_text_music():
    p = project(10)
    split_at(p, 4.0)
    delete_clip(p, 0)
    split_at(p, 3.0)
    p.music = MusicTrack(
        'C:/m/song.mp3', duration=100.0, offset=12.0, volume=0.5, fade_out=True, fade_seconds=2
    )
    from core.video.export import OverlayImage

    args = build_args(p, [OverlayImage('C:/t/0.png', 10, 20, 1.0, 2.5)], Path('C:/out/x.mp4'), 'mp4', '720p')
    fc = args[args.index('-filter_complex') + 1]
    assert 'trim=start=4.000:end=7.000' in fc and 'trim=start=7.000:end=10.000' in fc
    assert 'concat=n=2:v=1:a=1' in fc
    assert 'scale=1280:720' in fc
    assert "overlay=x=10:y=20:enable='between(t,1.000,2.500)'" in fc
    assert 'afade=t=out:st=4.000:d=2.000' in fc and 'amix=inputs=2' in fc
    assert args[args.index('-ss') + 1] == '12.000'
    assert '-movflags' in args and args[-1] == str(Path('C:/out/x.mp4'))


def test_build_args_mute_and_no_audio():
    p = project(10)
    p.mute = True
    args = build_args(p, [], Path('x.mov'), 'mov')
    fc = args[args.index('-filter_complex') + 1]
    assert '[0:a]' not in fc and '-an' in args and '-movflags' not in args
    q = project(10, audio=False)
    q.music = MusicTrack('m.wav', duration=5.0)
    args = build_args(q, [], Path('x.mp4'))
    assert '-map' in args and '[amus]' in args


def test_export_refuses_the_source(tmp_path):
    src = tmp_path / 'in.mp4'
    src.write_bytes(b'')
    p = VideoProject.new(src, info())
    with pytest.raises(ValueError):
        export_video(p, [], src)


# Real FFmpeg -----------------------------------------------------------------------------------


def _ffmpeg() -> str:
    try:
        return str(find_ffmpeg())
    except FFmpegNotFound:
        pytest.skip('FFmpeg not available')


@pytest.fixture(scope='module')
def media(tmp_path_factory):
    ff = _ffmpeg()
    d = tmp_path_factory.mktemp('video')
    video = d / 'clip thử.mp4'
    music = d / 'nhạc.wav'
    subprocess.run(
        [
            ff,
            '-y',
            '-loglevel',
            'error',
            '-f',
            'lavfi',
            '-i',
            'testsrc=size=320x240:rate=25:duration=4',
            '-f',
            'lavfi',
            '-i',
            'sine=frequency=440:duration=4',
            '-c:v',
            'libx264',
            '-pix_fmt',
            'yuv420p',
            '-c:a',
            'aac',
            '-shortest',
            str(video),
        ],
        check=True,
        **popen_kwargs(),
    )
    subprocess.run(
        [ff, '-y', '-loglevel', 'error', '-f', 'lavfi', '-i', 'sine=frequency=880:duration=10', str(music)],
        check=True,
        **popen_kwargs(),
    )
    return video, music


def test_probe_real_file(media):
    i = probe(media[0])
    assert i.has_video and i.has_audio and (i.width, i.height) == (320, 240)
    assert i.duration == pytest.approx(4.0, abs=0.1)


def test_export_cut_text_music_mov(media, tmp_path):
    from PIL import Image

    from core.video.export import OverlayImage

    video, music = media
    p = VideoProject.new(video, probe(video))
    split_at(p, 1.0)
    split_at(p, 2.0)
    delete_clip(p, 1)  # remove 1–2 s → 3 s left
    move_clip(p, 1, -1)
    p.music = MusicTrack(str(music), probe(music).duration, offset=1.0, volume=0.8)
    png = tmp_path / 't.png'
    Image.new('RGBA', (100, 30), (255, 0, 0, 200)).save(png)
    progress: list[float] = []
    out = export_video(
        p,
        [OverlayImage(str(png), 10, 10, 0.5, 2.0)],
        tmp_path / 'editor' / 'out.mov',
        'mov',
        on_progress=progress.append,
    )
    r = probe(out)
    assert r.duration == pytest.approx(3.0, abs=0.15)
    assert r.has_audio and (r.width, r.height) == (320, 240)
    assert progress and progress[-1] == 1.0
    assert not list((tmp_path / 'editor').glob('*.tmp.*'))
    assert video.exists()


def test_export_muted_mp4_without_sound(media, tmp_path):
    video, _ = media
    p = VideoProject.new(video, probe(video))
    p.mute = True
    out = export_video(p, [], tmp_path / 'm.mp4', 'mp4')
    assert not probe(out).has_audio


def test_export_cancel_leaves_no_file(media, tmp_path):
    video, _ = media
    p = VideoProject.new(video, probe(video))
    p.clips = [Clip(0, 4)] * 30  # long enough to cancel
    cancel = threading.Event()
    cancel.set()
    with pytest.raises(Cancelled):
        export_video(p, [], tmp_path / 'c.mp4', cancel_event=cancel)
    assert not (tmp_path / 'c.mp4').exists() and not list(tmp_path.glob('*.tmp.*'))


def test_overlay_png_matches_output_size(qapp, media, tmp_path):
    from ui.video.text_render import export_overlays

    video, _ = media
    p = VideoProject.new(video, probe(video))
    p.texts.append(TextOverlay(text='Xin chào', font_size=24, x=0.5, y=0.5, start=0.5, end=99))
    p.texts.append(TextOverlay(text='  ', start=0, end=1))  # empty: skipped
    ovs = export_overlays(p, 640, 480, tmp_path)
    assert len(ovs) == 1
    from PIL import Image

    with Image.open(ovs[0].path) as im:
        w, h = im.size
    assert ovs[0].end == pytest.approx(p.duration)
    assert ovs[0].x == pytest.approx(320 - w / 2, abs=1) and ovs[0].y == pytest.approx(240 - h / 2, abs=1)
    assert 40 < w < 400  # drawn at 2× (480 / 240)


def test_video_view_open_split_undo(qapp, media):
    from ui.video.video_editor_view import VideoEditorView

    video, _ = media
    v = VideoEditorView()
    v.resize(1300, 800)
    v.open_path(video, confirm=False)
    for _ in range(400):
        pump(0.02)
        if v.project is not None:
            break
    assert v.project is not None
    v.seek(2.0)
    v.split()
    assert len(v.project.clips) == 2 and v.modified
    v.delete_clip(0)
    assert v.project.duration == pytest.approx(2.0, abs=0.05)
    v.undo()
    v.undo()
    assert len(v.project.clips) == 1
    v.add_text()
    assert len(v.project.texts) == 1 and v.sel_text
    v.set_field('mute', True)
    assert v.project.mute
    v.modified = False
    v.shutdown()
