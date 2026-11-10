"""
Scanner recursif multithread.

Le parcours de l'arborescence (os.walk) est sequentiel (I/O rapide),
mais l'extraction des metadonnees de chaque fichier (stat, MIME) est
parallelisee via un ThreadPoolExecutor afin de ne jamais bloquer
l'interface, meme sur des dossiers contenant plusieurs centaines de
milliers de fichiers.
"""
from __future__ import annotations

import fnmatch
import os
import threading
from collections.abc import Callable
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

from core.categorizer import Categorizer
from core.models import FileInfo, ScanResult
from utils.config import Settings
from utils.helpers import guess_mime_type
from utils.logger import get_logger

logger = get_logger(__name__)

ProgressCallback = Callable[[int, int, str], None]  # (scanned, total_estime, chemin_courant)


class ScanCancelled(Exception):
    pass


class FolderScanner:
    """Analyse recursive d'un dossier avec extraction de metadonnees."""

    def __init__(self, settings: Settings, categorizer: Categorizer):
        self._settings = settings
        self._categorizer = categorizer
        self._cancel_event = threading.Event()

    def cancel(self) -> None:
        self._cancel_event.set()

    def reset(self) -> None:
        self._cancel_event.clear()

    def _is_excluded(self, path: Path) -> bool:
        for excluded in self._settings.excluded_folders:
            try:
                if excluded and Path(excluded).resolve() in path.resolve().parents:
                    return True
                if excluded and str(path.resolve()) == str(Path(excluded).resolve()):
                    return True
            except OSError:
                continue
        return False

    def _passes_filters(self, size: int, extension: str) -> bool:
        if extension.lower() in {e.lower() for e in self._settings.ignored_extensions}:
            return False
        if self._settings.min_size_bytes is not None and size < self._settings.min_size_bytes:
            return False
        if self._settings.max_size_bytes is not None and size > self._settings.max_size_bytes:
            return False
        return True

    def _build_file_info(self, path: Path) -> FileInfo | None:
        try:
            stat = path.stat()
        except OSError as exc:
            logger.debug("Impossible de lire %s : %s", path, exc)
            return None

        extension = path.suffix.lower()
        if not self._passes_filters(stat.st_size, extension):
            return None

        category = self._categorizer.categorize(extension)
        mime_type = guess_mime_type(path)

        return FileInfo(
            path=path,
            name=path.name,
            extension=extension,
            size=stat.st_size,
            created=getattr(stat, "st_birthtime", stat.st_ctime),
            modified=stat.st_mtime,
            accessed=stat.st_atime,
            mime_type=mime_type,
            category=category,
        )

    def scan(self, root: Path, progress_callback: ProgressCallback | None = None) -> ScanResult:
        """Lance une analyse complete et bloquante (a appeler depuis un thread)."""
        self.reset()
        result = ScanResult(root=root)

        # 1) Parcours de l'arborescence pour lister fichiers et dossiers.
        all_file_paths: list[Path] = []
        for dirpath, dirnames, filenames in os.walk(root):
            if self._cancel_event.is_set():
                result.cancelled = True
                return result

            current_dir = Path(dirpath)
            if self._is_excluded(current_dir):
                dirnames[:] = []
                continue

            # Empeche de descendre dans les dossiers exclus.
            dirnames[:] = [
                d for d in dirnames if not self._is_excluded(current_dir / d)
            ]

            result.folders.append(current_dir)
            for filename in filenames:
                all_file_paths.append(current_dir / filename)

        total = len(all_file_paths)
        scanned = 0

        # 2) Extraction parallele des metadonnees.
        max_workers = max(1, min(self._settings.max_workers, 32))
        with ThreadPoolExecutor(max_workers=max_workers) as executor:
            futures = {
                executor.submit(self._build_file_info, path): path
                for path in all_file_paths
            }
            for future in as_completed(futures):
                if self._cancel_event.is_set():
                    result.cancelled = True
                    for f in futures:
                        f.cancel()
                    break

                path = futures[future]
                scanned += 1
                try:
                    info = future.result()
                    if info is not None:
                        result.files.append(info)
                except Exception as exc:  # noqa: BLE001
                    result.errors.append(f"{path} : {exc}")

                if progress_callback and (scanned % 25 == 0 or scanned == total):
                    progress_callback(scanned, total, str(path))

        logger.info(
            "Scan termine : %d fichiers, %d dossiers, %d erreurs",
            result.total_files, result.total_folders, len(result.errors),
        )
        return result


def matches_any_pattern(name: str, patterns: list[str]) -> bool:
    return any(fnmatch.fnmatch(name.lower(), pattern.lower()) for pattern in patterns)
