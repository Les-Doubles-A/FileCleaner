"""
Configuration du systeme de journalisation (logs) de File Cleaner.

Tous les logs sont ecrits localement dans /logs, jamais transmis
a l'exterieur.
"""
from __future__ import annotations

import logging
import sys
from datetime import datetime
from pathlib import Path

LOG_DIR = Path(__file__).resolve().parent.parent / "logs"
LOG_DIR.mkdir(parents=True, exist_ok=True)

_LOG_FILE = LOG_DIR / f"filecleaner_{datetime.now():%Y%m%d}.log"

_FORMATTER = logging.Formatter(
    fmt="%(asctime)s | %(levelname)-8s | %(name)s | %(message)s",
    datefmt="%Y-%m-%d %H:%M:%S",
)

_configured = False


def _configure_root() -> None:
    global _configured
    if _configured:
        return

    root = logging.getLogger("filecleaner")
    root.setLevel(logging.DEBUG)

    file_handler = logging.FileHandler(_LOG_FILE, encoding="utf-8")
    file_handler.setFormatter(_FORMATTER)
    file_handler.setLevel(logging.DEBUG)

    console_handler = logging.StreamHandler(sys.stdout)
    console_handler.setFormatter(_FORMATTER)
    console_handler.setLevel(logging.INFO)

    root.addHandler(file_handler)
    root.addHandler(console_handler)
    _configured = True


def get_logger(name: str) -> logging.Logger:
    """Retourne un logger enfant nomme, partageant la configuration commune."""
    _configure_root()
    return logging.getLogger(f"filecleaner.{name}")


def current_log_file() -> Path:
    return _LOG_FILE
