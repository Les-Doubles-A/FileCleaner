"""
Analyses complementaires : gros fichiers, fichiers anciens,
dossiers vides et extensions rares.
"""
from __future__ import annotations

import os
from collections import Counter
from pathlib import Path

from core.models import FileInfo
from utils.helpers import days_since

MB = 1024 * 1024
GB = 1024 * MB


# ---------------------------------------------------------------------------
# Gros fichiers
# ---------------------------------------------------------------------------

BIG_FILE_PRESETS = {
    "100 Mo": 100 * MB,
    "500 Mo": 500 * MB,
    "1 Go": 1 * GB,
}


def find_big_files(files: list[FileInfo], min_size_bytes: int) -> list[FileInfo]:
    return sorted(
        (f for f in files if f.size >= min_size_bytes),
        key=lambda f: f.size,
        reverse=True,
    )


# ---------------------------------------------------------------------------
# Fichiers anciens
# ---------------------------------------------------------------------------

OLD_FILE_PRESETS_DAYS = {
    "1 an": 365,
    "2 ans": 730,
    "5 ans": 1825,
}


def find_old_files(files: list[FileInfo], min_days: int) -> list[FileInfo]:
    return sorted(
        (f for f in files if days_since(f.modified) >= min_days),
        key=lambda f: f.modified,
    )


# ---------------------------------------------------------------------------
# Dossiers vides
# ---------------------------------------------------------------------------

def find_empty_folders(folders: list[Path]) -> list[Path]:
    """
    Un dossier est considere comme vide s'il ne contient aucun fichier,
    y compris dans ses sous-dossiers (recursif "vide en profondeur").
    """
    empty: list[Path] = []
    for folder in sorted(folders, key=lambda p: len(p.parts), reverse=True):
        try:
            entries = list(os.scandir(folder))
        except OSError:
            continue
        if not entries:
            empty.append(folder)
    return empty


# ---------------------------------------------------------------------------
# Extensions rares
# ---------------------------------------------------------------------------

def find_rare_extensions(files: list[FileInfo], max_occurrences: int = 3) -> dict[str, list[FileInfo]]:
    """Retourne les extensions dont le nombre d'occurrences est <= max_occurrences."""
    counter: Counter[str] = Counter(f.extension or "(sans extension)" for f in files)
    rare_exts = {ext for ext, count in counter.items() if count <= max_occurrences}
    grouped: dict[str, list[FileInfo]] = {ext: [] for ext in rare_exts}
    for f in files:
        ext = f.extension or "(sans extension)"
        if ext in rare_exts:
            grouped[ext].append(f)
    return grouped
