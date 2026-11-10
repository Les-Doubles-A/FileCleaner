"""
Moteur de renommage intelligent.

Genere des noms de fichiers propres et uniformes, avec gestion des
conflits (numerotation automatique) et previsualisation obligatoire
avant toute application reelle sur le disque.
"""
from __future__ import annotations

import re
from dataclasses import dataclass

from core.models import FileInfo
from utils.helpers import sanitize_filename, strip_accents

# Reconnait des motifs de date usuels dans les noms de fichiers,
# ex: IMG_20260304_145021, 2026-03-04, 04-03-2026, Screenshot_20260304-145021
_DATE_PATTERNS = [
    re.compile(r"(?P<y>20\d{2})[-_.]?(?P<m>[01]\d)[-_.]?(?P<d>[0-3]\d)"),
]

_CAMERA_PREFIXES = re.compile(
    r"^(IMG|IMAGE|DSC|DCIM|PHOTO|VID|VIDEO|SCREENSHOT|SCR|PXL|WA)[_\-]?",
    re.IGNORECASE,
)
_TRAILING_COUNTER = re.compile(r"^(?P<base>.*?)\s*\(\d+\)$")


@dataclass
class RenamePlanItem:
    file: FileInfo
    new_name: str
    enabled: bool = True

    @property
    def new_path(self):
        return self.file.path.parent / self.new_name

    @property
    def changed(self) -> bool:
        return self.new_name != self.file.name


def extract_date_from_name(name: str) -> str | None:
    for pattern in _DATE_PATTERNS:
        match = pattern.search(name)
        if match:
            return f"{match.group('y')}-{match.group('m')}-{match.group('d')}"
    return None


def clean_stem(stem: str, keep_accents: bool = True) -> str:
    """Nettoie la partie 'nom' (sans extension) d'un fichier."""
    cleaned = _CAMERA_PREFIXES.sub("", stem)
    trailing = _TRAILING_COUNTER.match(cleaned)
    if trailing:
        cleaned = trailing.group("base")
    cleaned = sanitize_filename(cleaned, keep_accents=keep_accents)
    if not keep_accents:
        cleaned = strip_accents(cleaned)
    # Retire les longues suites de chiffres/heures type "20260304_145021"
    cleaned = re.sub(r"\b\d{6,}\b", "", cleaned).strip(" -_")
    cleaned = re.sub(r"\s{2,}", " ", cleaned).strip()
    return cleaned


def build_new_name(
    file: FileInfo,
    index: int,
    *,
    label: str | None = None,
    use_date_prefix: bool = True,
    keep_accents: bool = True,
    pad_width: int = 3,
) -> str:
    """
    Construit un nom de fichier propre.

    - Si un `label` est fourni (ex: "Vacances_Paris"), il est utilise
      comme base : "Vacances_Paris_001.jpg".
    - Sinon, tente d'extraire une date du nom d'origine et l'utilise
      comme base : "2026-03-04_001.jpg".
    - En dernier recours, nettoie simplement le nom d'origine.
    """
    extension = file.extension
    detected_date = extract_date_from_name(file.name) if use_date_prefix else None

    if label:
        base = sanitize_filename(label, keep_accents=keep_accents).replace(" ", "_")
    elif detected_date:
        base = detected_date
    else:
        cleaned = clean_stem(file.path.stem, keep_accents=keep_accents)
        base = cleaned.replace(" ", "_") if cleaned else "fichier"

    suffix = str(index).zfill(pad_width)
    return f"{base}_{suffix}{extension}"


def build_rename_plan(
    files: list[FileInfo],
    *,
    label: str | None = None,
    use_date_prefix: bool = True,
    keep_accents: bool = True,
    start_index: int = 1,
) -> list[RenamePlanItem]:
    """
    Construit un plan de renommage previsualisable (aucune ecriture disque).
    Evite les collisions de noms au sein du meme plan.
    """
    plan: list[RenamePlanItem] = []
    used_names_per_folder: dict[str, set[str]] = {}

    for offset, file in enumerate(files):
        index = start_index + offset
        new_name = build_new_name(
            file, index, label=label, use_date_prefix=use_date_prefix, keep_accents=keep_accents
        )

        folder_key = str(file.path.parent)
        used = used_names_per_folder.setdefault(folder_key, set())

        final_name = new_name
        bump = index
        while final_name.lower() in used or (
            (file.path.parent / final_name).exists() and final_name != file.name
        ):
            bump += 1
            stem, ext = new_name.rsplit(".", 1) if "." in new_name else (new_name, "")
            base_stem = re.sub(r"_\d+$", "", stem)
            final_name = f"{base_stem}_{str(bump).zfill(3)}" + (f".{ext}" if ext else "")

        used.add(final_name.lower())
        plan.append(RenamePlanItem(file=file, new_name=final_name))

    return plan
