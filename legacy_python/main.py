"""
File Cleaner - Point d'entree de l'application.

Lance l'interface graphique CustomTkinter. Fonctionne 100% en local,
aucune connexion reseau n'est requise ni utilisee.
"""
from __future__ import annotations

import sys
from pathlib import Path

# Permet l'execution directe (python main.py) en ajoutant la racine au path.
sys.path.insert(0, str(Path(__file__).resolve().parent))

from utils.logger import get_logger  # noqa: E402
from ui.app import FileCleanerApp  # noqa: E402

logger = get_logger(__name__)


def main() -> None:
    logger.info("Demarrage de File Cleaner")
    app = FileCleanerApp()
    app.mainloop()
    logger.info("Fermeture de File Cleaner")


if __name__ == "__main__":
    main()
