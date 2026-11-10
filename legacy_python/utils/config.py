"""
Gestion des parametres utilisateur de File Cleaner.

Les parametres sont stockes localement dans /config/settings.json.
Aucune donnee n'est jamais envoyee a l'exterieur.
"""
from __future__ import annotations

import json
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any

CONFIG_DIR = Path(__file__).resolve().parent.parent / "config"
CONFIG_DIR.mkdir(parents=True, exist_ok=True)
SETTINGS_FILE = CONFIG_DIR / "settings.json"


@dataclass
class Settings:
    # Dossiers/extensions ignores lors du scan
    excluded_folders: list[str] = field(default_factory=list)
    ignored_extensions: list[str] = field(default_factory=list)

    # Filtres de taille (en octets), None = pas de limite
    min_size_bytes: int | None = None
    max_size_bytes: int | None = None

    # Apparence
    theme: str = "dark"          # "dark" | "light" | "system"
    language: str = "fr"         # "fr" | "en"

    # Comportement
    ask_confirmation: bool = True
    use_recycle_bin: bool = True          # sinon -> dossier de quarantaine
    quarantine_folder: str | None = None  # None = dossier par defaut

    # Seuils "gros fichiers" / "anciens fichiers" par defaut (en Mo / jours)
    big_file_threshold_mb: int = 100
    old_file_threshold_days: int = 365

    # Nombre de threads pour le scan
    max_workers: int = 8

    # Categories activees et leurs extensions (modifiable par l'utilisateur)
    categories: dict[str, list[str]] = field(default_factory=dict)
    disabled_categories: list[str] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> "Settings":
        valid_keys = {f for f in cls.__dataclass_fields__}
        filtered = {k: v for k, v in data.items() if k in valid_keys}
        return cls(**filtered)


DEFAULT_CATEGORIES: dict[str, list[str]] = {
    "Images": [".jpg", ".jpeg", ".png", ".gif", ".bmp", ".webp", ".svg", ".heic", ".tiff", ".ico"],
    "Videos": [".mp4", ".mkv", ".avi", ".mov", ".wmv", ".flv", ".webm", ".m4v", ".mpg", ".mpeg"],
    "Audio": [".mp3", ".wav", ".flac", ".aac", ".ogg", ".wma", ".m4a"],
    "Documents": [".doc", ".docx", ".odt", ".txt", ".rtf", ".xls", ".xlsx", ".ods",
                  ".ppt", ".pptx", ".odp", ".csv"],
    "PDF": [".pdf"],
    "Archives": [".zip", ".rar", ".7z", ".tar", ".gz", ".bz2", ".xz", ".iso"],
    "Code": [".py", ".js", ".ts", ".html", ".css", ".java", ".c", ".cpp", ".h",
             ".cs", ".php", ".rb", ".go", ".rs", ".json", ".xml", ".yaml", ".yml", ".sql"],
    "Applications": [".exe", ".msi", ".apk", ".app", ".dmg", ".bat", ".sh"],
    "Bases de donnees": [".db", ".sqlite", ".sqlite3", ".mdb", ".accdb", ".sql"],
    "Polices": [".ttf", ".otf", ".woff", ".woff2"],
    "E-books": [".epub", ".mobi", ".azw", ".azw3", ".fb2"],
    "Modeles 3D": [".obj", ".fbx", ".blend", ".stl", ".3ds", ".gltf", ".glb"],
    "Divers": [],
}

JUNK_PATTERNS: list[str] = [
    "*.tmp", "*.bak", "*.old", "*.cache", "*.log", "*.dmp",
    "Thumbs.db", "Desktop.ini", "*.~*", ".DS_Store", "*.swp",
]


def load_settings() -> Settings:
    if SETTINGS_FILE.exists():
        try:
            data = json.loads(SETTINGS_FILE.read_text(encoding="utf-8"))
            settings = Settings.from_dict(data)
        except (json.JSONDecodeError, OSError):
            settings = Settings()
    else:
        settings = Settings()

    if not settings.categories:
        settings.categories = {k: list(v) for k, v in DEFAULT_CATEGORIES.items()}

    return settings


def save_settings(settings: Settings) -> None:
    SETTINGS_FILE.write_text(
        json.dumps(settings.to_dict(), indent=2, ensure_ascii=False),
        encoding="utf-8",
    )
