"""
Fonctions utilitaires generiques utilisees dans tout le projet :
formatage de tailles, dates, detection MIME, hachage, sanitation de noms.
"""
from __future__ import annotations

import hashlib
import mimetypes
import re
import unicodedata
from datetime import datetime
from pathlib import Path

# ---------------------------------------------------------------------------
# Tailles et dates
# ---------------------------------------------------------------------------

_SIZE_UNITS = ["o", "Ko", "Mo", "Go", "To", "Po"]


def format_size(size_bytes: int) -> str:
    """Formate une taille en octets en chaine lisible (ex: 12,4 Mo)."""
    if size_bytes < 0:
        size_bytes = 0
    size = float(size_bytes)
    for unit in _SIZE_UNITS:
        if size < 1024.0 or unit == _SIZE_UNITS[-1]:
            if unit == "o":
                return f"{int(size)} {unit}"
            return f"{size:.2f} {unit}".replace(".", ",")
        size /= 1024.0
    return f"{size:.2f} Po"


def format_datetime(ts: float) -> str:
    """Formate un timestamp epoch en chaine JJ/MM/AAAA HH:MM."""
    try:
        return datetime.fromtimestamp(ts).strftime("%d/%m/%Y %H:%M")
    except (OSError, OverflowError, ValueError):
        return "Inconnue"


def days_since(ts: float) -> int:
    """Nombre de jours ecoules depuis le timestamp donne."""
    try:
        delta = datetime.now() - datetime.fromtimestamp(ts)
        return max(delta.days, 0)
    except (OSError, OverflowError, ValueError):
        return 0


# ---------------------------------------------------------------------------
# Hachage
# ---------------------------------------------------------------------------

def sha256_of_file(path: Path, chunk_size: int = 1024 * 1024) -> str:
    """Calcule le hash SHA256 complet d'un fichier."""
    hasher = hashlib.sha256()
    with open(path, "rb") as f:
        while True:
            chunk = f.read(chunk_size)
            if not chunk:
                break
            hasher.update(chunk)
    return hasher.hexdigest()


def quick_partial_hash(path: Path, sample_size: int = 65536) -> str:
    """
    Hash rapide et partiel (debut + fin du fichier) utilise comme pre-filtre
    avant de calculer le hash SHA256 complet, pour accelerer la detection
    de doublons sur de gros volumes de fichiers.
    """
    hasher = hashlib.md5()
    size = path.stat().st_size
    with open(path, "rb") as f:
        hasher.update(f.read(sample_size))
        if size > sample_size:
            f.seek(max(size - sample_size, 0))
            hasher.update(f.read(sample_size))
    return hasher.hexdigest()


# ---------------------------------------------------------------------------
# MIME
# ---------------------------------------------------------------------------

def guess_mime_type(path: Path) -> str:
    mime, _ = mimetypes.guess_type(str(path))
    return mime or "application/octet-stream"


# ---------------------------------------------------------------------------
# Noms de fichiers
# ---------------------------------------------------------------------------

_INVALID_CHARS = re.compile(r'[<>:"/\\|?*\x00-\x1f]')
_MULTI_SEP = re.compile(r"[ _\-]{2,}")
_MULTI_SPACE = re.compile(r"\s{2,}")


def strip_accents(text: str) -> str:
    normalized = unicodedata.normalize("NFKD", text)
    return "".join(c for c in normalized if not unicodedata.combining(c))


def sanitize_filename(name: str, keep_accents: bool = True) -> str:
    """Nettoie un nom de fichier : caracteres invalides, espaces multiples, etc."""
    cleaned = _INVALID_CHARS.sub("", name)
    if not keep_accents:
        cleaned = strip_accents(cleaned)
    cleaned = cleaned.replace("_", " ")
    cleaned = _MULTI_SPACE.sub(" ", cleaned)
    cleaned = cleaned.strip(" .-_")
    return cleaned or "fichier"


def unique_path(target: Path) -> Path:
    """
    Retourne un chemin non-conflictuel : si `target` existe deja, ajoute
    un suffixe numerique ' (1)', ' (2)', etc. avant l'extension.
    """
    if not target.exists():
        return target
    stem, suffix, parent = target.stem, target.suffix, target.parent
    counter = 1
    while True:
        candidate = parent / f"{stem} ({counter}){suffix}"
        if not candidate.exists():
            return candidate
        counter += 1
