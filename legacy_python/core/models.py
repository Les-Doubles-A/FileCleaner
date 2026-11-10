"""
Structures de donnees centrales de File Cleaner.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path


@dataclass
class FileInfo:
    """Metadonnees d'un fichier scanne."""

    path: Path
    name: str
    extension: str
    size: int
    created: float
    modified: float
    accessed: float
    mime_type: str
    category: str = "Divers"

    # Calcules a la demande (couteux), remplis uniquement quand necessaire
    sha256: str | None = None
    partial_hash: str | None = None

    @property
    def is_junk_candidate(self) -> bool:
        return False  # determine par analysis.junk, laisse ici pour extension future

    def to_export_dict(self) -> dict:
        return {
            "chemin": str(self.path),
            "nom": self.name,
            "extension": self.extension,
            "taille_octets": self.size,
            "cree": datetime.fromtimestamp(self.created).isoformat(timespec="seconds"),
            "modifie": datetime.fromtimestamp(self.modified).isoformat(timespec="seconds"),
            "accede": datetime.fromtimestamp(self.accessed).isoformat(timespec="seconds"),
            "type_mime": self.mime_type,
            "categorie": self.category,
            "sha256": self.sha256 or "",
        }


@dataclass
class ScanResult:
    """Resultat complet d'une analyse de dossier."""

    root: Path
    files: list[FileInfo] = field(default_factory=list)
    folders: list[Path] = field(default_factory=list)
    scanned_at: datetime = field(default_factory=datetime.now)
    errors: list[str] = field(default_factory=list)
    cancelled: bool = False

    @property
    def total_files(self) -> int:
        return len(self.files)

    @property
    def total_folders(self) -> int:
        return len(self.folders)

    @property
    def total_size(self) -> int:
        return sum(f.size for f in self.files)

    def extension_breakdown(self) -> dict[str, tuple[int, int]]:
        """Retourne {extension: (nombre_fichiers, taille_totale)}."""
        breakdown: dict[str, tuple[int, int]] = {}
        for f in self.files:
            ext = f.extension or "(sans extension)"
            count, size = breakdown.get(ext, (0, 0))
            breakdown[ext] = (count + 1, size + f.size)
        return breakdown

    def category_breakdown(self) -> dict[str, tuple[int, int]]:
        breakdown: dict[str, tuple[int, int]] = {}
        for f in self.files:
            count, size = breakdown.get(f.category, (0, 0))
            breakdown[f.category] = (count + 1, size + f.size)
        return breakdown

    def largest_files(self, n: int = 20) -> list[FileInfo]:
        return sorted(self.files, key=lambda f: f.size, reverse=True)[:n]

    def oldest_files(self, n: int = 20) -> list[FileInfo]:
        return sorted(self.files, key=lambda f: f.modified)[:n]
