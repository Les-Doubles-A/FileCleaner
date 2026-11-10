"""
Detection des fichiers inutiles / indesirables :
temporaires, cache, logs, miniatures, sauvegardes, etc.
"""
from __future__ import annotations

import fnmatch

from core.models import FileInfo
from utils.config import JUNK_PATTERNS

_THUMBNAIL_DIR_HINTS = {"thumbnails", ".thumbnails", "cache", ".cache"}


def is_junk_file(file: FileInfo, patterns: list[str] | None = None) -> bool:
    patterns = patterns or JUNK_PATTERNS
    if any(fnmatch.fnmatch(file.name.lower(), p.lower()) for p in patterns):
        return True
    parent_name = file.path.parent.name.lower()
    if parent_name in _THUMBNAIL_DIR_HINTS:
        return True
    return False


def find_junk_files(files: list[FileInfo], patterns: list[str] | None = None) -> list[FileInfo]:
    return [f for f in files if is_junk_file(f, patterns)]


def total_junk_size(files: list[FileInfo], patterns: list[str] | None = None) -> int:
    return sum(f.size for f in find_junk_files(files, patterns))
