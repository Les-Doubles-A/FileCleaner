"""
Historique des operations effectuees par File Cleaner.

Chaque session d'analyse/nettoyage est enregistree localement dans
/history/history.json : dossier analyse, fichiers supprimes, deplaces,
renommes. Les operations reversibles (deplacement, quarantaine,
renommage) peuvent etre restaurees.
"""
from __future__ import annotations

import json
import shutil
import uuid
from dataclasses import asdict, dataclass, field
from datetime import datetime
from pathlib import Path

from utils.logger import get_logger

logger = get_logger(__name__)

HISTORY_DIR = Path(__file__).resolve().parent.parent / "history"
HISTORY_DIR.mkdir(parents=True, exist_ok=True)
HISTORY_FILE = HISTORY_DIR / "history.json"


@dataclass
class FileOperation:
    """Une operation atomique sur un fichier (pour restauration)."""

    kind: str            # "delete" | "move" | "rename" | "quarantine"
    source: str
    destination: str | None = None   # ou se trouve le fichier apres l'operation
    restorable: bool = True
    restored: bool = False


@dataclass
class OperationSession:
    """Un ensemble d'operations effectuees lors d'une meme action de nettoyage."""

    id: str = field(default_factory=lambda: uuid.uuid4().hex[:12])
    timestamp: str = field(default_factory=lambda: datetime.now().isoformat(timespec="seconds"))
    scanned_folder: str = ""
    label: str = ""              # description courte, ex: "Suppression de doublons"
    operations: list[FileOperation] = field(default_factory=list)

    @property
    def files_deleted(self) -> int:
        return sum(1 for op in self.operations if op.kind in ("delete", "quarantine"))

    @property
    def files_moved(self) -> int:
        return sum(1 for op in self.operations if op.kind == "move")

    @property
    def files_renamed(self) -> int:
        return sum(1 for op in self.operations if op.kind == "rename")


class HistoryManager:
    def __init__(self):
        self._sessions: list[OperationSession] = []
        self._load()

    def _load(self) -> None:
        if not HISTORY_FILE.exists():
            self._sessions = []
            return
        try:
            raw = json.loads(HISTORY_FILE.read_text(encoding="utf-8"))
            self._sessions = [
                OperationSession(
                    id=s["id"],
                    timestamp=s["timestamp"],
                    scanned_folder=s.get("scanned_folder", ""),
                    label=s.get("label", ""),
                    operations=[FileOperation(**op) for op in s.get("operations", [])],
                )
                for s in raw
            ]
        except (json.JSONDecodeError, OSError, KeyError) as exc:
            logger.warning("Historique illisible, reinitialisation : %s", exc)
            self._sessions = []

    def _save(self) -> None:
        data = [
            {
                "id": s.id,
                "timestamp": s.timestamp,
                "scanned_folder": s.scanned_folder,
                "label": s.label,
                "operations": [asdict(op) for op in s.operations],
            }
            for s in self._sessions
        ]
        HISTORY_FILE.write_text(json.dumps(data, indent=2, ensure_ascii=False), encoding="utf-8")

    def new_session(self, scanned_folder: str, label: str) -> OperationSession:
        session = OperationSession(scanned_folder=scanned_folder, label=label)
        self._sessions.append(session)
        return session

    def commit(self, session: OperationSession) -> None:
        self._save()
        logger.info(
            "Session '%s' enregistree (%d operations)", session.label, len(session.operations)
        )

    def all_sessions(self) -> list[OperationSession]:
        return sorted(self._sessions, key=lambda s: s.timestamp, reverse=True)

    def restore_session(self, session_id: str) -> tuple[int, int]:
        """
        Tente de restaurer toutes les operations reversibles d'une session.
        Retourne (nombre_restaure, nombre_echoue).
        """
        session = next((s for s in self._sessions if s.id == session_id), None)
        if session is None:
            return 0, 0

        restored, failed = 0, 0
        for op in session.operations:
            if op.restored or not op.restorable or op.destination is None:
                continue
            try:
                dest = Path(op.destination)
                src = Path(op.source)
                if not dest.exists():
                    failed += 1
                    continue
                src.parent.mkdir(parents=True, exist_ok=True)
                shutil.move(str(dest), str(src))
                op.restored = True
                restored += 1
            except OSError as exc:
                logger.warning("Restauration impossible pour %s : %s", op.source, exc)
                failed += 1

        self._save()
        return restored, failed
