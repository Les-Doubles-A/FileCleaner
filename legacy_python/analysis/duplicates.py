"""
Detection des doublons.

Strategie en 3 etapes pour rester performant sur de gros volumes :
1. Regroupement par taille (les fichiers de taille differente ne peuvent
   pas etre identiques).
2. Pour chaque groupe de taille >= 2 fichiers, hash partiel (debut+fin)
   pour ecarter rapidement les faux positifs.
3. Hash SHA256 complet uniquement pour confirmer les candidats restants.

Une detection separee par nom identique est aussi proposee (utile pour
reperer "Copie", "Copie (1)", "Copie finale", meme si le contenu differe).
"""
from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass, field

from core.models import FileInfo
from utils.helpers import quick_partial_hash, sha256_of_file
from utils.logger import get_logger

logger = get_logger(__name__)


@dataclass
class DuplicateGroup:
    """Un ensemble de fichiers consideres comme doublons entre eux."""

    key: str                      # hash sha256 (ou nom, selon le mode)
    files: list[FileInfo] = field(default_factory=list)

    @property
    def total_size(self) -> int:
        return sum(f.size for f in self.files)

    @property
    def recoverable_size(self) -> int:
        """Espace recuperable si on ne garde qu'un seul exemplaire."""
        if not self.files:
            return 0
        return self.total_size - max(f.size for f in self.files)

    def suggest_original(self, strategy: str = "oldest") -> FileInfo:
        """
        Suggere quel fichier conserver selon une strategie :
        'oldest' (plus ancien), 'newest' (plus recent),
        'largest' (plus grand), 'smallest' (plus petit).
        """
        if strategy == "newest":
            return max(self.files, key=lambda f: f.modified)
        if strategy == "largest":
            return max(self.files, key=lambda f: f.size)
        if strategy == "smallest":
            return min(self.files, key=lambda f: f.size)
        return min(self.files, key=lambda f: f.modified)  # oldest par defaut


def find_duplicates_by_content(files: list[FileInfo]) -> list[DuplicateGroup]:
    """Detecte les doublons par contenu reel (hash SHA256 confirme)."""
    by_size: dict[int, list[FileInfo]] = defaultdict(list)
    for f in files:
        if f.size > 0:  # les fichiers vides ne sont pas consideres comme doublons utiles
            by_size[f.size].append(f)

    size_candidates = [group for group in by_size.values() if len(group) >= 2]

    by_partial: dict[tuple[int, str], list[FileInfo]] = defaultdict(list)
    for group in size_candidates:
        for f in group:
            try:
                f.partial_hash = quick_partial_hash(f.path)
                by_partial[(f.size, f.partial_hash)].append(f)
            except OSError as exc:
                logger.debug("Hash partiel impossible pour %s : %s", f.path, exc)

    partial_candidates = [group for group in by_partial.values() if len(group) >= 2]

    by_sha256: dict[str, list[FileInfo]] = defaultdict(list)
    for group in partial_candidates:
        for f in group:
            try:
                f.sha256 = sha256_of_file(f.path)
                by_sha256[f.sha256].append(f)
            except OSError as exc:
                logger.debug("Hash SHA256 impossible pour %s : %s", f.path, exc)

    groups = [
        DuplicateGroup(key=digest, files=members)
        for digest, members in by_sha256.items()
        if len(members) >= 2
    ]
    groups.sort(key=lambda g: g.recoverable_size, reverse=True)
    logger.info("Doublons par contenu detectes : %d groupes", len(groups))
    return groups


def find_duplicates_by_name(files: list[FileInfo]) -> list[DuplicateGroup]:
    """Detecte les fichiers portant exactement le meme nom (dans des dossiers differents)."""
    by_name: dict[str, list[FileInfo]] = defaultdict(list)
    for f in files:
        by_name[f.name.lower()].append(f)

    groups = [
        DuplicateGroup(key=name, files=members)
        for name, members in by_name.items()
        if len(members) >= 2
    ]
    groups.sort(key=lambda g: len(g.files), reverse=True)
    return groups


def find_duplicates_by_size_only(files: list[FileInfo]) -> list[DuplicateGroup]:
    """Regroupement rapide (sans hash) par taille identique, utile en pre-visualisation."""
    by_size: dict[int, list[FileInfo]] = defaultdict(list)
    for f in files:
        if f.size > 0:
            by_size[f.size].append(f)
    groups = [
        DuplicateGroup(key=str(size), files=members)
        for size, members in by_size.items()
        if len(members) >= 2
    ]
    groups.sort(key=lambda g: g.total_size, reverse=True)
    return groups
