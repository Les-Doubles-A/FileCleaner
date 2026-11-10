"""
Classement automatique des fichiers en categories (Images, Videos, ...)
en fonction de leur extension, selon les regles definies dans les
parametres utilisateur (modifiables : accepter / modifier / desactiver).
"""
from __future__ import annotations

from utils.config import Settings


class Categorizer:
    """Associe une extension de fichier a une categorie utilisateur."""

    def __init__(self, settings: Settings):
        self._settings = settings
        self._ext_to_category: dict[str, str] = {}
        self._rebuild_index()

    def _rebuild_index(self) -> None:
        self._ext_to_category.clear()
        for category, extensions in self._settings.categories.items():
            if category in self._settings.disabled_categories:
                continue
            for ext in extensions:
                self._ext_to_category[ext.lower()] = category

    def refresh(self, settings: Settings) -> None:
        """A appeler si les parametres ont change (categories editees)."""
        self._settings = settings
        self._rebuild_index()

    def categorize(self, extension: str) -> str:
        return self._ext_to_category.get(extension.lower(), "Divers")

    def category_names(self) -> list[str]:
        return [c for c in self._settings.categories if c not in self._settings.disabled_categories]
