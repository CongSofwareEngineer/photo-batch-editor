"""App version: ``version.json`` at the project root (bundled next to the app by the spec).

``version`` (``X.Y.Z``) is edited by hand for a release; ``build`` is increased by
``tools/bump_build.py`` on every build_windows.bat / build_mac.sh run.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path

from core.paths import resource_path

VERSION_FILE = 'version.json'


@dataclass(frozen=True)
class AppVersion:
    version: str = '0.0.0'
    build: int = 0

    @property
    def numbers(self) -> tuple[int, int, int, int]:
        """``(X, Y, Z, build)`` for the Windows file version resource."""
        parts = [int(p) if p.isdigit() else 0 for p in self.version.split('.')[:3]]
        parts += [0] * (3 - len(parts))
        return parts[0], parts[1], parts[2], self.build

    @property
    def full(self) -> str:
        """``1.0.0.12``"""
        return '.'.join(str(n) for n in self.numbers)


def read_version(path: Path | None = None) -> AppVersion:
    """Version of the running app (source or bundle); ``0.0.0`` if the file is missing/broken."""
    try:
        data = json.loads((path or resource_path(VERSION_FILE)).read_text(encoding='utf-8'))
        return AppVersion(str(data.get('version', '0.0.0')), int(data.get('build', 0)))
    except (OSError, ValueError, TypeError, AttributeError):
        return AppVersion()


def write_version(path: Path, ver: AppVersion) -> None:
    data = {'version': ver.version, 'build': ver.build}
    path.write_text(json.dumps(data, indent=2) + '\n', encoding='utf-8')
