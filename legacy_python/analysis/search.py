"""
Recherche et filtrage sur l'ensemble des fichiers analyses.
"""
from __future__ import annotations

from dataclasses import dataclass

from core.models import FileInfo
from utils.helpers import days_since


@dataclass
class SearchCriteria:
    text: str | None = None                 # recherche dans le nom
    extensions: list[str] | None = None      # ex: [".jpg", ".png"]
    categories: list[str] | None = None      # ex: ["Images", "Videos"]
    min_size_bytes: int | None = None
    max_size_bytes: int | None = None
    modified_after_days: int | None = None   # modifie il y a moins de N jours
    modified_before_days: int | None = None  # modifie il y a plus de N jours
    folder: str | None = None                # sous-chaine de chemin


def search_files(files: list[FileInfo], criteria: SearchCriteria) -> list[FileInfo]:
    results = files

    if criteria.text:
        needle = criteria.text.lower()
        results = [f for f in results if needle in f.name.lower()]

    if criteria.extensions:
        allowed = {e.lower() for e in criteria.extensions}
        results = [f for f in results if f.extension.lower() in allowed]

    if criteria.categories:
        allowed_cats = set(criteria.categories)
        results = [f for f in results if f.category in allowed_cats]

    if criteria.min_size_bytes is not None:
        results = [f for f in results if f.size >= criteria.min_size_bytes]

    if criteria.max_size_bytes is not None:
        results = [f for f in results if f.size <= criteria.max_size_bytes]

    if criteria.modified_after_days is not None:
        results = [f for f in results if days_since(f.modified) <= criteria.modified_after_days]

    if criteria.modified_before_days is not None:
        results = [f for f in results if days_since(f.modified) >= criteria.modified_before_days]

    if criteria.folder:
        needle_folder = criteria.folder.lower()
        results = [f for f in results if needle_folder in str(f.path.parent).lower()]

    return results
