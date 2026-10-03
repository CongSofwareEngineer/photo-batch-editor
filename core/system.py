"""Operating-system differences (Windows 10/11, macOS, Linux) in one place. No Qt."""

from __future__ import annotations

import sys

IS_WINDOWS = sys.platform == 'win32'
IS_MAC = sys.platform == 'darwin'

# Default font of new text layers (photo editor) and video texts: installed on every machine of
# that OS and has the Vietnamese accents.
if IS_WINDOWS:
    DEFAULT_FONT_FAMILY = 'Segoe UI'
elif IS_MAC:
    DEFAULT_FONT_FAMILY = 'Helvetica Neue'
else:
    DEFAULT_FONT_FAMILY = 'DejaVu Sans'
