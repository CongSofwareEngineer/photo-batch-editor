"""Batch processing: CPU process pool or 3-stage GPU pipeline (spec sections 9, 10.5).

CLI (temporary, phases 1–3)::

    python -m core.batch "D:\\Shoot\\clientA" --preset presets_builtin/warm.json --device cpu
"""

from __future__ import annotations

import json
import logging
import multiprocessing
import os
import queue
import sys
import threading
import time
from collections.abc import Callable
from concurrent.futures import FIRST_COMPLETED, Future, ProcessPoolExecutor, wait
from concurrent.futures.process import BrokenProcessPool
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any

import cv2

from core.backend import Backend, detect_gpu, get_backend, get_cached_gpu_info, get_cpu_backend
from core.enhance import SuperResolver
from core.io_utils import ImageReadError, LoadedImage, read_image, tmp_path_for, write_jpeg
from core.pipeline import process_image
from core.scanner import Job, default_output_dir, plan_jobs, scan_folder
from core.settings import AdjustmentSettings

log = logging.getLogger(__name__)

VRAM_FACTOR = 6  # bytes needed ≈ H * W * 3 * 4 * VRAM_FACTOR
VRAM_MAX_FRACTION = 0.8
MAX_GPU_FAILURES = 3
GPU_FALLBACK_WARNING = 'Photo too large for VRAM, processed on CPU'
GPU_ERROR_WARNING = 'GPU error, processed on CPU'
CORRUPT_FILE = 'Corrupt file'
LOG_FILENAME = 'process_log.txt'


@dataclass
class FileResult:
    input_path: Path
    output_path: Path | None
    rel_dir: str
    status: str  # "ok" | "warning" | "error" | "cancelled"
    message: str
    width: int | None = None
    height: int | None = None
    size_bytes: int | None = None
    seconds: float = 0.0
    device: str = 'cpu'  # device that actually processed this photo


@dataclass
class BatchReport:
    input_dir: Path
    output_dir: Path
    cancelled: bool
    device_requested: str
    device_used: str  # "gpu" | "cpu" | "mixed"
    device_name: str
    total_seconds: float
    files: list[FileResult] = field(default_factory=list)

    def count(self, *statuses: str) -> int:
        return sum(1 for f in self.files if f.status in statuses)

    @property
    def succeeded(self) -> int:
        return self.count('ok', 'warning')


ProgressCallback = Callable[[int, int, str, FileResult | None], None]


def _error(job: Job, message: str, seconds: float, device: str) -> FileResult:
    return FileResult(job.input_path, None, job.rel_dir, 'error', message, seconds=seconds, device=device)


def _finish(
    job: Job, out_pixels: Any, warnings: list[str], loaded: LoadedImage, t0: float, device: str
) -> FileResult:
    """Write the JPEG and build the result."""
    warnings = list(warnings) + write_jpeg(job.output_path, out_pixels, loaded.exif, loaded.icc)
    h, w = out_pixels.shape[:2]
    notes = list(dict.fromkeys(warnings))
    status = 'warning' if notes else 'ok'
    if job.renamed:
        notes.append(f'Saved as {job.output_path.name} (name collision)')
    return FileResult(
        job.input_path,
        job.output_path,
        job.rel_dir,
        status,
        '; '.join(notes),
        int(w),
        int(h),
        job.output_path.stat().st_size,
        time.perf_counter() - t0,
        device,
    )


def process_job(
    job: Job, settings: AdjustmentSettings, backend: Backend, sr: SuperResolver | None = None
) -> FileResult:
    """Read, process and write one photo. Never raises."""
    t0 = time.perf_counter()
    if job.input_path.resolve() == job.output_path.resolve():
        return _error(job, 'Output would overwrite the original', 0.0, backend.name)
    try:
        loaded = read_image(job.input_path)
    except ImageReadError as exc:
        return _error(job, str(exc) or CORRUPT_FILE, time.perf_counter() - t0, backend.name)
    except Exception as exc:  # noqa: BLE001
        return _error(job, f'{CORRUPT_FILE}: {exc}', time.perf_counter() - t0, backend.name)
    try:
        out, warnings = process_image(loaded.pixels, settings, backend, sr)
        return _finish(job, out, warnings, loaded, t0, backend.name)
    except Exception as exc:  # noqa: BLE001
        log.exception('Failed: %s', job.input_path)
        return _error(job, f'Processing failed: {exc}', time.perf_counter() - t0, backend.name)


# CPU mode: process pool ------------------------------------------------------------------


def _worker_init(threads: int) -> None:
    logging.basicConfig(level=logging.WARNING)
    try:
        cv2.setNumThreads(max(1, threads))
    except Exception:  # noqa: BLE001
        pass


def _cpu_worker(job: Job, settings_dict: dict) -> FileResult:
    """Runs in a worker process: reads, processes and writes the file itself."""
    return process_job(job, AdjustmentSettings.from_dict(settings_dict), get_cpu_backend())


def default_cpu_workers(settings: AdjustmentSettings) -> int:
    if settings.sr_factor > 1:
        return 1  # ONNX Runtime already uses all cores
    return min(max(1, (os.cpu_count() or 2) - 1), 4)


def _terminate_pool(ex: ProcessPoolExecutor) -> None:
    procs = list((getattr(ex, '_processes', None) or {}).values())
    ex.shutdown(wait=False, cancel_futures=True)
    for p in procs:
        try:
            p.terminate()
        except Exception:  # noqa: BLE001
            pass
    for p in procs:
        try:
            p.join(timeout=5)
        except Exception:  # noqa: BLE001
            pass


def _run_cpu(
    jobs: list[Job],
    settings: AdjustmentSettings,
    results: list[FileResult | None],
    emit: Callable[[str, FileResult | None], None],
    cancel: threading.Event,
    workers: int | None,
) -> None:
    n = workers or default_cpu_workers(settings)
    n = max(1, min(n, len(jobs)))
    threads = max(1, (os.cpu_count() or 2) // n)
    ex = ProcessPoolExecutor(
        max_workers=n,
        mp_context=multiprocessing.get_context('spawn'),
        initializer=_worker_init,
        initargs=(threads,),
    )
    settings_dict = settings.to_dict()
    pending: dict[Future, int] = {}
    next_idx = 0

    def submit_more() -> None:
        nonlocal next_idx
        while next_idx < len(jobs) and len(pending) < n + 1 and not cancel.is_set():
            pending[ex.submit(_cpu_worker, jobs[next_idx], settings_dict)] = next_idx
            next_idx += 1

    broken = False
    try:
        submit_more()
        if pending:
            emit(jobs[min(pending.values())].input_path.name, None)
        while pending and not cancel.is_set():
            done, _ = wait(list(pending), timeout=0.2, return_when=FIRST_COMPLETED)
            for fut in done:
                idx = pending.pop(fut)
                try:
                    res = fut.result()
                except BrokenProcessPool:
                    broken = True  # retried in-process below
                    continue
                except Exception as exc:  # noqa: BLE001
                    res = _error(jobs[idx], f'Processing failed: {exc}', 0.0, 'cpu')
                results[idx] = res
                emit(jobs[idx].input_path.name, res)
            if broken:
                break
            submit_more()
    except BrokenProcessPool:
        broken = True
    finally:
        if cancel.is_set() or broken:
            # Finished futures keep their result; in-flight photos are discarded.
            for fut, idx in pending.items():
                if fut.done() and not fut.cancelled() and fut.exception() is None:
                    results[idx] = fut.result()
                    emit(jobs[idx].input_path.name, results[idx])
            _terminate_pool(ex)
        else:
            ex.shutdown(wait=True)

    if broken:
        # A worker process died (e.g. out of memory) or could not start: one failing photo
        # never stops the batch, so the remaining photos run in this process.
        log.warning('Process pool broke; processing the remaining photos in-process')
        cpu = get_cpu_backend()
        for idx, job in enumerate(jobs):
            if cancel.is_set():
                break
            if results[idx] is None:
                emit(job.input_path.name, None)
                results[idx] = process_job(job, settings, cpu)
                emit(job.input_path.name, results[idx])


# GPU mode: reader threads → GPU thread → writer threads -----------------------------------

_STOP = object()


def _run_gpu(
    jobs: list[Job],
    settings: AdjustmentSettings,
    gpu: Backend,
    results: list[FileResult | None],
    emit: Callable[[str, FileResult | None], None],
    cancel: threading.Event,
    on_device_change: Callable[[str], None] | None,
) -> None:
    cpu = get_cpu_backend()
    read_q: queue.Queue = queue.Queue(maxsize=2)
    write_q: queue.Queue = queue.Queue(maxsize=2)
    next_job = iter(range(len(jobs)))
    job_lock = threading.Lock()
    n_readers, n_writers = 2, 2

    def reader() -> None:
        try:
            while not cancel.is_set():
                with job_lock:
                    idx = next(next_job, None)
                if idx is None:
                    break
                t0 = time.perf_counter()
                try:
                    item: Any = read_image(jobs[idx].input_path)
                except Exception as exc:  # noqa: BLE001
                    item = exc
                read_q.put((idx, item, t0))
        finally:
            read_q.put(_STOP)

    failures = 0
    use_gpu = True

    def fail(idx: int, message: str, t0: float, device: str) -> None:
        results[idx] = _error(jobs[idx], message, time.perf_counter() - t0, device)
        emit(jobs[idx].input_path.name, results[idx])

    def gpu_photo(idx: int, item: Any, t0: float) -> None:
        """Process one photo on the GPU (or on the CPU as fallback) and queue it for writing."""
        nonlocal failures, use_gpu
        job = jobs[idx]
        if isinstance(item, Exception):
            text = str(item) if isinstance(item, ImageReadError) else f'{CORRUPT_FILE}: {item}'
            fail(idx, text or CORRUPT_FILE, t0, 'gpu')
            return
        if cancel.is_set():
            return  # read but not processed: stays "cancelled"
        emit(job.input_path.name, None)
        backend, extra = (gpu, []) if use_gpu else (cpu, [])
        if use_gpu:
            h, w = item.pixels.shape[:2]
            need = h * w * 3 * 4 * VRAM_FACTOR
            avail = gpu_available_bytes(gpu)
            if avail is not None and need > VRAM_MAX_FRACTION * avail:
                backend, extra = cpu, [GPU_FALLBACK_WARNING]
        try:
            out, warnings = process_image(item.pixels, settings, backend)
            if backend is gpu:
                failures = 0
        except Exception as exc:  # noqa: BLE001 - OOM or any CUDA error
            if backend is not gpu:
                fail(idx, f'Processing failed: {exc}', t0, backend.name)
                return
            log.warning('GPU failed on %s: %s; retrying on CPU', job.input_path.name, exc)
            gpu.free_memory()
            failures += 1
            if failures >= MAX_GPU_FAILURES and use_gpu:
                use_gpu = False
                if on_device_change:
                    on_device_change('cpu')
            backend, extra = cpu, [GPU_FALLBACK_WARNING]
            try:
                out, warnings = process_image(item.pixels, settings, cpu)
            except Exception as exc2:  # noqa: BLE001
                fail(idx, f'Processing failed: {exc2}', t0, 'cpu')
                return
        write_q.put((idx, out, extra + warnings, item, t0, backend.name))

    def gpu_worker() -> None:
        stops = 0
        try:
            while stops < n_readers:  # always drained, so readers never block on a full queue
                msg = read_q.get()
                if msg is _STOP:
                    stops += 1
                    continue
                idx, item, t0 = msg
                try:
                    gpu_photo(idx, item, t0)
                except Exception as exc:  # noqa: BLE001 - one photo never stops the batch
                    log.exception('Unexpected error on %s', jobs[idx].input_path)
                    fail(idx, f'Processing failed: {exc}', t0, 'gpu')
        finally:
            for _ in range(n_writers):
                write_q.put(_STOP)

    def writer() -> None:
        while True:
            msg = write_q.get()
            if msg is _STOP:
                break
            idx, out, warnings, loaded, t0, device = msg
            try:
                res = _finish(jobs[idx], out, warnings, loaded, t0, device)
            except Exception as exc:  # noqa: BLE001
                res = _error(jobs[idx], f'Write failed: {exc}', time.perf_counter() - t0, device)
            results[idx] = res
            emit(jobs[idx].input_path.name, res)

    threads = [threading.Thread(target=reader, name=f'reader-{i}', daemon=True) for i in range(n_readers)]
    threads.append(threading.Thread(target=gpu_worker, name='gpu', daemon=True))
    threads += [threading.Thread(target=writer, name=f'writer-{i}', daemon=True) for i in range(n_writers)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    gpu.free_memory()
    gpu.set_memory_limit(None)


def gpu_available_bytes(backend: Backend) -> int | None:
    """Free VRAM plus memory cached in the CuPy pool (reusable)."""
    info = backend.mem_info()
    if info is None:
        return None
    pool_free = getattr(backend, 'pool_free_bytes', lambda: 0)()
    return info[0] + pool_free


# Public API ------------------------------------------------------------------------------


def run_batch(
    input_dir: Path,
    settings: AdjustmentSettings,
    device: str = 'gpu',
    output_dir: Path | None = None,
    on_progress: ProgressCallback | None = None,
    cancel_event: threading.Event | None = None,
    workers: int | None = None,
    on_device_change: Callable[[str], None] | None = None,
    backend: Backend | None = None,
    files: list[Path] | None = None,
) -> BatchReport:
    """Process every photo of ``input_dir`` into ``output_dir`` (default ``<name>_update``)."""
    start = time.perf_counter()
    input_dir = Path(input_dir).resolve()
    output_dir = Path(output_dir) if output_dir else default_output_dir(input_dir)
    files = scan_folder(input_dir) if files is None else files
    jobs = plan_jobs(input_dir, files, output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    cancel = cancel_event or threading.Event()
    backend = backend or get_backend(device)
    results: list[FileResult | None] = [None] * len(jobs)
    lock = threading.Lock()
    done_count = 0

    def emit(current: str, res: FileResult | None) -> None:
        nonlocal done_count
        with lock:
            if res is not None:
                done_count += 1
            if on_progress:
                try:
                    on_progress(done_count, len(jobs), current, res)
                except Exception:  # noqa: BLE001
                    log.exception('on_progress callback failed')

    if jobs:
        if backend.name == 'gpu':
            _run_gpu(jobs, settings, backend, results, emit, cancel, on_device_change)
        else:
            _run_cpu(jobs, settings, results, emit, cancel, workers)

    files_out: list[FileResult] = []
    for job, res in zip(jobs, results, strict=True):
        if res is None:
            res = FileResult(job.input_path, None, job.rel_dir, 'cancelled', 'Cancelled', device=backend.name)
        files_out.append(res)
    for job in jobs:  # never leave half-written files behind
        tmp = tmp_path_for(job.output_path)
        if tmp.exists():
            try:
                tmp.unlink()
            except OSError:
                log.warning('Could not delete %s', tmp)

    used = {r.device for r in files_out if r.status in ('ok', 'warning')}
    device_used = used.pop() if len(used) == 1 else ('mixed' if used else backend.name)
    report = BatchReport(
        input_dir,
        output_dir,
        cancel.is_set(),
        device,
        device_used,
        backend.device_name,
        time.perf_counter() - start,
        files_out,
    )
    write_process_log(report, settings)
    return report


def device_description(report: BatchReport) -> str:
    if report.device_used == 'cpu':
        return get_cpu_backend().device_name
    info = get_cached_gpu_info()
    name = report.device_name
    if info and info.driver_version:
        name += f' (driver {info.driver_version})'
    return name + (' + CPU fallback' if report.device_used == 'mixed' else '')


def write_process_log(report: BatchReport, settings: AdjustmentSettings) -> None:
    """``process_log.txt`` in the output folder (section 9)."""
    lines = [
        'Photo Batch Editor — process log',
        f'Date: {datetime.now().isoformat(timespec="seconds")}',
        f'Input folder: {report.input_dir}',
        f'Output folder: {report.output_dir}',
        f'Device requested: {report.device_requested}',
        f'Device used: {report.device_used} — {device_description(report)}',
        f'Run time: {report.total_seconds:.1f} s',
        f'Cancelled: {"yes" if report.cancelled else "no"}',
        f'Result: {report.succeeded}/{len(report.files)} succeeded, '
        f'{report.count("warning")} with warnings, {report.count("error")} failed, '
        f'{report.count("cancelled")} cancelled',
        '',
        'Settings:',
        json.dumps(settings.to_dict(), indent=2),
        '',
        'Files:',
    ]
    for r in report.files:
        rel = os.path.relpath(r.input_path, report.input_dir)
        out = r.output_path.name if r.output_path else '-'
        size = f'{r.width}x{r.height}' if r.width else '-'
        lines.append(
            f'[{r.status.upper():9}] {rel} -> {out} | {size} | {r.device} | {r.seconds:.2f} s | {r.message}'
        )
    try:
        (Path(report.output_dir) / LOG_FILENAME).write_text('\n'.join(lines) + '\n', encoding='utf-8')
    except OSError:
        log.warning('Could not write %s', LOG_FILENAME, exc_info=True)


def _cli() -> None:
    import argparse  # noqa: PLC0415

    from core import (
        batch as batch_mod,  # noqa: PLC0415 - pickle-able functions under core.batch
    )
    from core.presets import load_preset  # noqa: PLC0415
    from core.scanner import next_free_output_dir  # noqa: PLC0415

    for stream in (sys.stdout, sys.stderr):  # Vietnamese file names on a cp1252 console
        reconfigure = getattr(stream, 'reconfigure', None)
        if reconfigure is None:
            continue
        try:
            reconfigure(encoding='utf-8', errors='replace')
        except ValueError:
            pass
    logging.basicConfig(level=logging.INFO, format='%(levelname)s %(message)s')
    p = argparse.ArgumentParser(prog='python -m core.batch', description='Photo Batch Editor CLI')
    p.add_argument('folder', type=Path)
    p.add_argument('--preset', type=Path, help='preset .json file')
    p.add_argument('--device', choices=('gpu', 'cpu'), default='gpu')
    p.add_argument('--super-resolution', choices=('off', '2x', '4x'), default=None)
    p.add_argument(
        '--if-exists',
        choices=('new', 'overwrite'),
        default='new',
        help='when <folder>_update exists: create _update_2… (default) or overwrite',
    )
    p.add_argument('--workers', type=int, default=None)
    a = p.parse_args()

    settings = load_preset(a.preset).settings if a.preset else AdjustmentSettings()
    if a.super_resolution:
        settings.super_resolution = a.super_resolution
    out = default_output_dir(a.folder)
    if out.exists() and a.if_exists == 'new':
        out = next_free_output_dir(a.folder)
    if a.device == 'gpu':
        info = detect_gpu()
        print(
            f'GPU: {info.name} ({(info.vram_total or 0) / 2**30:.1f} GB)'
            if info.available
            else f'GPU unavailable — using CPU. Reason: {info.reason}'
        )

    def progress(done: int, total: int, current: str, res: FileResult | None) -> None:
        if res is not None:
            mark = {'ok': 'OK  ', 'warning': 'WARN', 'error': 'ERR ', 'cancelled': 'CANC'}[res.status]
            print(f'[{done}/{total}] {mark} {current} {("- " + res.message) if res.message else ""}')

    report = batch_mod.run_batch(a.folder, settings, a.device, out, progress, workers=a.workers)
    print()
    print(f'Output:       {report.output_dir}')
    print(f'Device used:  {report.device_used} ({device_description(report)})')
    print(f'Total time:   {report.total_seconds:.1f} s')
    print(
        f'Succeeded:    {report.succeeded}/{len(report.files)} '
        f'({report.count("warning")} with warnings), failed: {report.count("error")}'
    )


if __name__ == '__main__':
    multiprocessing.freeze_support()
    _cli()
