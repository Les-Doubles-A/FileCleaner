"""
Moteur d'execution des actions de nettoyage.

Principes de securite :
- Aucune suppression definitive automatique. Par defaut, les fichiers
  supprimes passent par la corbeille systeme (send2trash) ou, a defaut,
  par un dossier de quarantaine local reversible.
- Chaque operation est journalisee via HistoryManager pour permettre
  une restauration ulterieure.
- Toute action destructrice necessite un plan explicite prealablement
  valide par l'utilisateur (aucune execution "silencieuse").
"""
from __future__ import annotations

import shutil
from dataclasses import dataclass, field
from pathlib import Path

from cleaner.rename_engine import RenamePlanItem
from core.models import FileInfo
from history.history_manager import FileOperation, HistoryManager, OperationSession
from utils.config import Settings
from utils.helpers import unique_path
from utils.logger import get_logger

logger = get_logger(__name__)

try:
    from send2trash import send2trash
    _HAS_SEND2TRASH = True
except ImportError:  # pragma: no cover - depend de l'environnement d'execution
    _HAS_SEND2TRASH = False


DEFAULT_QUARANTINE_DIRNAME = ".filecleaner_quarantine"


@dataclass
class ActionError:
    path: str
    message: str


@dataclass
class ActionReport:
    session: OperationSession
    succeeded: int = 0
    failed: list[ActionError] = field(default_factory=list)
    freed_bytes: int = 0


class ActionEngine:
    """Execute les plans de nettoyage valides par l'utilisateur."""

    def __init__(self, settings: Settings, history: HistoryManager):
        self._settings = settings
        self._history = history

    # ------------------------------------------------------------------
    # Quarantaine
    # ------------------------------------------------------------------

    def _quarantine_root(self, scanned_folder: Path) -> Path:
        if self._settings.quarantine_folder:
            root = Path(self._settings.quarantine_folder)
        else:
            root = scanned_folder / DEFAULT_QUARANTINE_DIRNAME
        root.mkdir(parents=True, exist_ok=True)
        return root

    # ------------------------------------------------------------------
    # Suppression (doublons, fichiers inutiles, gros fichiers, etc.)
    # ------------------------------------------------------------------

    def delete_files(
        self,
        files: list[FileInfo],
        scanned_folder: Path,
        label: str = "Suppression de fichiers",
    ) -> ActionReport:
        session = self._history.new_session(str(scanned_folder), label)
        report = ActionReport(session=session)
        quarantine_root = self._quarantine_root(scanned_folder)

        for file in files:
            src = file.path
            try:
                if self._settings.use_recycle_bin and _HAS_SEND2TRASH:
                    send2trash(str(src))
                    op = FileOperation(kind="delete", source=str(src), destination=None, restorable=False)
                else:
                    dest = unique_path(quarantine_root / src.name)
                    shutil.move(str(src), str(dest))
                    op = FileOperation(kind="quarantine", source=str(src), destination=str(dest))

                session.operations.append(op)
                report.succeeded += 1
                report.freed_bytes += file.size
            except OSError as exc:
                logger.warning("Suppression impossible pour %s : %s", src, exc)
                report.failed.append(ActionError(path=str(src), message=str(exc)))

        self._history.commit(session)
        return report

    # ------------------------------------------------------------------
    # Deplacement / classement automatique par categorie
    # ------------------------------------------------------------------

    def move_files(
        self,
        mapping: list[tuple[FileInfo, Path]],
        scanned_folder: Path,
        label: str = "Classement automatique",
    ) -> ActionReport:
        """`mapping` est une liste de (FileInfo, dossier_destination)."""
        session = self._history.new_session(str(scanned_folder), label)
        report = ActionReport(session=session)

        for file, dest_folder in mapping:
            src = file.path
            try:
                dest_folder.mkdir(parents=True, exist_ok=True)
                dest = unique_path(dest_folder / src.name)
                shutil.move(str(src), str(dest))
                session.operations.append(
                    FileOperation(kind="move", source=str(src), destination=str(dest))
                )
                report.succeeded += 1
            except OSError as exc:
                logger.warning("Deplacement impossible pour %s : %s", src, exc)
                report.failed.append(ActionError(path=str(src), message=str(exc)))

        self._history.commit(session)
        return report

    # ------------------------------------------------------------------
    # Renommage
    # ------------------------------------------------------------------

    def apply_rename_plan(
        self,
        plan: list[RenamePlanItem],
        scanned_folder: Path,
        label: str = "Renommage intelligent",
    ) -> ActionReport:
        session = self._history.new_session(str(scanned_folder), label)
        report = ActionReport(session=session)

        for item in plan:
            if not item.enabled or not item.changed:
                continue
            src = item.file.path
            dest = item.new_path
            try:
                dest = unique_path(dest) if dest.exists() else dest
                src.rename(dest)
                session.operations.append(
                    FileOperation(kind="rename", source=str(src), destination=str(dest))
                )
                item.file.path = dest
                item.file.name = dest.name
                report.succeeded += 1
            except OSError as exc:
                logger.warning("Renommage impossible pour %s : %s", src, exc)
                report.failed.append(ActionError(path=str(src), message=str(exc)))

        self._history.commit(session)
        return report

    # ------------------------------------------------------------------
    # Dossiers vides
    # ------------------------------------------------------------------

    def delete_empty_folders(
        self,
        folders: list[Path],
        scanned_folder: Path,
        label: str = "Suppression de dossiers vides",
    ) -> ActionReport:
        session = self._history.new_session(str(scanned_folder), label)
        report = ActionReport(session=session)

        # Traite les dossiers les plus profonds en premier.
        for folder in sorted(folders, key=lambda p: len(p.parts), reverse=True):
            try:
                folder.rmdir()
                session.operations.append(
                    FileOperation(kind="delete", source=str(folder), destination=None, restorable=False)
                )
                report.succeeded += 1
            except OSError as exc:
                logger.debug("Suppression de dossier impossible pour %s : %s", folder, exc)
                report.failed.append(ActionError(path=str(folder), message=str(exc)))

        self._history.commit(session)
        return report
