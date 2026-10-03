"""Folder scanning, output folder and output file naming (spec sections 4, 7.2, 7.4)."""

from __future__ import annotations

import logging
import os
import stat
from dataclasses import dataclass
from pathlib import Path

log = logging.getLogger(__name__)

SUPPORTED_EXTENSIONS = frozenset({'.jpg', '.jpeg', '.png', '.tif', '.tiff', '.webp', '.bmp'})
SKIPPED_NAMES = frozenset({'thumbs.db', 'desktop.ini'})
OUTPUT_SUFFIX = '_update'


class OutputFolderError(ValueError):
    """The input folder cannot be used (e.g. a drive root)."""


@dataclass
class Job:
    """One input file and where its JPEG will be written."""

    input_path: Path
    output_path: Path
    rel_dir: str  # sub-folder relative to the input folder, "" at the root
    renamed: bool = False  # output name changed because of a name collision


def _is_hidden(entry: os.DirEntry) -> bool:
    if entry.name.startswith('.'):
        return True
    try:
        attrs = getattr(entry.stat(follow_symlinks=False), 'st_file_attributes', 0)
    except OSError:
        return True
    return bool(
        attrs & (getattr(stat, 'FILE_ATTRIBUTE_HIDDEN', 2) | getattr(stat, 'FILE_ATTRIBUTE_SYSTEM', 4))
    )


def is_supported(name: str) -> bool:
    return Path(name).suffix.lower() in SUPPORTED_EXTENSIONS


def _sort_key(rel: Path) -> tuple[str, ...]:
    return tuple(part.casefold() for part in rel.parts)


def scan_folder(folder: Path) -> list[Path]:
    """All supported images under ``folder`` (recursive), ordered by relative path."""
    folder = Path(folder)
    found: list[Path] = []
    stack = [folder]
    while stack:
        current = stack.pop()
        try:
            with os.scandir(current) as it:
                entries = list(it)
        except OSError as exc:
            log.warning('Cannot read folder %s: %s', current, exc)
            continue
        for entry in entries:
            if _is_hidden(entry) or entry.name.casefold() in SKIPPED_NAMES:
                continue
            try:
                if entry.is_dir(follow_symlinks=False):
                    stack.append(Path(entry.path))
                elif entry.is_file() and is_supported(entry.name):
                    found.append(Path(entry.path))
            except OSError:
                continue
    found.sort(key=lambda p: _sort_key(p.relative_to(folder)))
    return found


def is_drive_root(folder: Path) -> bool:
    folder = Path(folder).resolve()
    return folder.parent == folder


def default_output_dir(input_dir: Path) -> Path:
    """Sibling folder ``<name>_update``; drive roots are rejected (section 7.2)."""
    input_dir = Path(input_dir).resolve()
    if is_drive_root(input_dir):
        raise OutputFolderError('Cannot process a drive root. Please choose a folder.')
    return input_dir.parent / f'{input_dir.name}{OUTPUT_SUFFIX}'


def next_free_output_dir(input_dir: Path) -> Path:
    """``<name>_update`` if free, else ``<name>_update_2``, ``_3``…"""
    base = default_output_dir(input_dir)
    if not base.exists():
        return base
    n = 2
    while True:
        candidate = base.with_name(f'{base.name}_{n}')
        if not candidate.exists():
            return candidate
        n += 1


def plan_jobs(input_dir: Path, files: list[Path], output_dir: Path) -> list[Job]:
    """Compute every output path up front so two workers never write the same name.

    Name collisions in one folder (``a.png`` + ``a.jpg``): sorted by full file name,
    the first keeps ``a.jpg``, the next become ``a_1.jpg``, ``a_2.jpg``…
    """
    input_dir = Path(input_dir)
    by_dir: dict[str, list[Path]] = {}
    for f in files:
        rel_dir = f.parent.relative_to(input_dir).as_posix()
        by_dir.setdefault('' if rel_dir == '.' else rel_dir, []).append(f)

    planned: dict[Path, Job] = {}
    for rel_dir, group in by_dir.items():
        used: set[str] = set()  # case-insensitive, like Windows file names
        for f in sorted(group, key=lambda p: p.name.casefold()):
            stem = f.stem
            name = f'{stem}.jpg'
            n = 0
            while name.casefold() in used:
                n += 1
                name = f'{stem}_{n}.jpg'
            used.add(name.casefold())
            renamed = n > 0
            out = Path(output_dir).joinpath(*([p for p in rel_dir.split('/') if p]), name)
            planned[f] = Job(f, out, rel_dir, renamed)
    return [planned[f] for f in files]
