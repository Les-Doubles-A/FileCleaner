"""
Fenetre principale de File Cleaner.

Orchestre le scan (multithread, non bloquant), les differentes analyses
et les actions de nettoyage, via une interface a onglets.
"""
from __future__ import annotations

import threading
from pathlib import Path
from tkinter import filedialog, messagebox

import customtkinter as ctk

from analysis import duplicates as dup_analysis
from analysis import filters as filter_analysis
from analysis import junk as junk_analysis
from analysis.search import SearchCriteria, search_files
from cleaner.action_engine import ActionEngine
from cleaner.rename_engine import build_rename_plan
from core.categorizer import Categorizer
from core.models import FileInfo, ScanResult
from core.scanner import FolderScanner
from export import exporter
from history.history_manager import HistoryManager
from ui.dialogs import ConfirmDeleteDialog, RenamePreviewDialog
from ui.settings_dialog import SettingsDialog
from ui.widgets import DropZone, FileTable, FolderList, configure_treeview_style
from utils.config import Settings, load_settings
from utils.helpers import format_size
from utils.logger import get_logger

logger = get_logger(__name__)

ctk.set_appearance_mode("dark")
ctk.set_default_color_theme("blue")

ICON_FILENAME = "FileCleaner.ico"


def _resolve_app_base_class():
    """
    Determine la classe de base a utiliser pour la fenetre principale.

    Si tkinterdnd2 est installe, on combine CTk avec le mixin DnD de
    tkinterdnd2 (technique standard pour obtenir un glisser-deposer natif
    reel dans une application CustomTkinter). Sinon, on retombe sur CTk
    seul : la zone de glisser-deposer proposera alors le bouton "Choisir
    un dossier" comme repli.
    """
    try:
        from tkinterdnd2 import TkinterDnD

        class _CTkDnD(ctk.CTk, TkinterDnD.DnDWrapper):
            def __init__(self, *args, **kwargs):
                super().__init__(*args, **kwargs)
                self.TkdndVersion = TkinterDnD._require(self)

        return _CTkDnD
    except Exception:
        return ctk.CTk


_AppBase = _resolve_app_base_class()


class FileCleanerApp(_AppBase):
    def __init__(self):
        super().__init__()
        self.title("File Cleaner")
        self.geometry("1280x800")
        self.minsize(1080, 680)

        self._settings: Settings = load_settings()
        ctk.set_appearance_mode(self._settings.theme)
        configure_treeview_style(self._settings.theme if self._settings.theme != "system" else "dark")

        self._categorizer = Categorizer(self._settings)
        self._scanner = FolderScanner(self._settings, self._categorizer)
        self._history = HistoryManager()
        self._action_engine = ActionEngine(self._settings, self._history)

        self._current_folder: Path | None = None
        self._scan_result: ScanResult | None = None
        self._duplicate_groups: list[dup_analysis.DuplicateGroup] = []
        self._scan_thread: threading.Thread | None = None

        self._apply_icon()
        self._build_layout()

    # ------------------------------------------------------------------
    # Icone
    # ------------------------------------------------------------------

    def _apply_icon(self) -> None:
        icon_path = Path(__file__).resolve().parent.parent / "icons" / ICON_FILENAME
        if icon_path.exists():
            try:
                self.iconbitmap(str(icon_path))
            except Exception as exc:  # pragma: no cover - depend de l'OS
                logger.debug("Impossible de definir l'icone : %s", exc)
        else:
            logger.debug("Icone %s introuvable dans /icons (attendue pour la version packagee).", icon_path)

    # ------------------------------------------------------------------
    # Layout general
    # ------------------------------------------------------------------

    def _build_layout(self) -> None:
        self.grid_columnconfigure(1, weight=1)
        self.grid_rowconfigure(0, weight=1)

        self._build_sidebar()
        self._build_main_area()

    def _build_sidebar(self) -> None:
        sidebar = ctk.CTkFrame(self, width=300, corner_radius=0)
        sidebar.grid(row=0, column=0, sticky="nsw")
        sidebar.grid_propagate(False)

        ctk.CTkLabel(
            sidebar, text="File Cleaner", font=ctk.CTkFont(size=22, weight="bold")
        ).pack(padx=20, pady=(24, 4), anchor="w")
        ctk.CTkLabel(
            sidebar, text="Analyse et nettoyage 100% local", text_color="gray60"
        ).pack(padx=20, pady=(0, 16), anchor="w")

        self._drop_zone = DropZone(sidebar, on_folder_dropped=self._on_folder_selected, height=110)
        self._drop_zone.pack(fill="x", padx=20, pady=8)

        ctk.CTkButton(sidebar, text="Choisir un dossier", command=self._browse_folder).pack(
            fill="x", padx=20, pady=6
        )

        self._folder_label = ctk.CTkLabel(
            sidebar, text="Aucun dossier selectionne", text_color="gray60", wraplength=260, justify="left"
        )
        self._folder_label.pack(fill="x", padx=20, pady=(4, 12))

        self._scan_button = ctk.CTkButton(
            sidebar, text="Lancer l'analyse", command=self._start_scan, state="disabled"
        )
        self._scan_button.pack(fill="x", padx=20, pady=6)

        self._cancel_button = ctk.CTkButton(
            sidebar, text="Annuler", fg_color="gray40", command=self._cancel_scan, state="disabled"
        )
        self._cancel_button.pack(fill="x", padx=20, pady=6)

        self._progress_bar = ctk.CTkProgressBar(sidebar)
        self._progress_bar.set(0)
        self._progress_bar.pack(fill="x", padx=20, pady=(16, 4))
        self._progress_label = ctk.CTkLabel(sidebar, text="", text_color="gray60")
        self._progress_label.pack(fill="x", padx=20)

        ctk.CTkButton(sidebar, text="Parametres", fg_color="gray30", command=self._open_settings).pack(
            fill="x", padx=20, pady=(24, 6), side="bottom"
        )

        export_row = ctk.CTkFrame(sidebar, fg_color="transparent")
        export_row.pack(fill="x", padx=20, pady=6, side="bottom")
        ctk.CTkLabel(export_row, text="Exporter :").pack(side="left")
        for label, ext in (("CSV", "csv"), ("JSON", "json"), ("PDF", "pdf")):
            ctk.CTkButton(
                export_row, text=label, width=54, command=lambda e=ext: self._export(e)
            ).pack(side="left", padx=2)

        ctk.CTkLabel(sidebar, text="Journal", font=ctk.CTkFont(weight="bold")).pack(
            anchor="w", padx=20, pady=(16, 4), side="bottom"
        )

    def _build_main_area(self) -> None:
        container = ctk.CTkFrame(self, fg_color="transparent")
        container.grid(row=0, column=1, sticky="nsew", padx=16, pady=16)
        container.grid_rowconfigure(0, weight=3)
        container.grid_rowconfigure(1, weight=1)
        container.grid_columnconfigure(0, weight=1)

        self._tabview = ctk.CTkTabview(container)
        self._tabview.grid(row=0, column=0, sticky="nsew")

        self._tab_summary = self._tabview.add("Resume")
        self._tab_duplicates = self._tabview.add("Doublons")
        self._tab_junk = self._tabview.add("Fichiers inutiles")
        self._tab_big = self._tabview.add("Gros fichiers")
        self._tab_old = self._tabview.add("Anciens fichiers")
        self._tab_empty = self._tabview.add("Dossiers vides")
        self._tab_rare = self._tabview.add("Extensions rares")
        self._tab_sort = self._tabview.add("Classement et renommage")
        self._tab_search = self._tabview.add("Recherche")
        self._tab_history = self._tabview.add("Historique")

        self._build_summary_tab()
        self._build_duplicates_tab()
        self._build_junk_tab()
        self._build_big_files_tab()
        self._build_old_files_tab()
        self._build_empty_folders_tab()
        self._build_rare_extensions_tab()
        self._build_sort_rename_tab()
        self._build_search_tab()
        self._build_history_tab()

        log_frame = ctk.CTkFrame(container)
        log_frame.grid(row=1, column=0, sticky="nsew", pady=(12, 0))
        log_frame.grid_columnconfigure(0, weight=1)
        log_frame.grid_rowconfigure(1, weight=1)
        ctk.CTkLabel(log_frame, text="Journal des operations", font=ctk.CTkFont(weight="bold")).grid(
            row=0, column=0, sticky="w", padx=10, pady=(8, 2)
        )
        self._log_box = ctk.CTkTextbox(log_frame, height=140, state="disabled")
        self._log_box.grid(row=1, column=0, sticky="nsew", padx=10, pady=(0, 10))

    # ------------------------------------------------------------------
    # Journal
    # ------------------------------------------------------------------

    def log(self, message: str) -> None:
        self._log_box.configure(state="normal")
        self._log_box.insert("end", f"{message}\n")
        self._log_box.see("end")
        self._log_box.configure(state="disabled")
        logger.info(message)

    # ------------------------------------------------------------------
    # Selection de dossier
    # ------------------------------------------------------------------

    def _browse_folder(self) -> None:
        folder = filedialog.askdirectory(title="Choisir un dossier a analyser")
        if folder:
            self._on_folder_selected(folder)

    def _on_folder_selected(self, folder: str) -> None:
        path = Path(folder)
        if not path.is_dir():
            messagebox.showerror("Dossier invalide", f"Le chemin n'est pas un dossier valide :\n{folder}")
            return
        self._current_folder = path
        self._folder_label.configure(text=str(path))
        self._scan_button.configure(state="normal")
        self.log(f"Dossier selectionne : {path}")

    # ------------------------------------------------------------------
    # Scan
    # ------------------------------------------------------------------

    def _start_scan(self) -> None:
        if not self._current_folder:
            return
        self._categorizer.refresh(self._settings)
        self._scan_button.configure(state="disabled")
        self._cancel_button.configure(state="normal")
        self._progress_bar.set(0)
        self.log(f"Analyse commencee : {self._current_folder}")

        self._scan_thread = threading.Thread(target=self._run_scan, daemon=True)
        self._scan_thread.start()

    def _run_scan(self) -> None:
        def on_progress(scanned: int, total: int, current_path: str) -> None:
            self.after(0, self._update_progress, scanned, total)

        result = self._scanner.scan(self._current_folder, progress_callback=on_progress)
        self.after(0, self._on_scan_finished, result)

    def _update_progress(self, scanned: int, total: int) -> None:
        ratio = (scanned / total) if total else 0
        self._progress_bar.set(ratio)
        self._progress_label.configure(text=f"{scanned}/{total} fichiers analyses")

    def _cancel_scan(self) -> None:
        self._scanner.cancel()
        self.log("Annulation demandee...")

    def _on_scan_finished(self, result: ScanResult) -> None:
        self._cancel_button.configure(state="disabled")
        self._scan_button.configure(state="normal")

        if result.cancelled:
            self.log("Analyse annulee.")
            self._progress_label.configure(text="Analyse annulee")
            return

        self._scan_result = result
        self._progress_bar.set(1)
        self._progress_label.configure(text=f"{result.total_files} fichiers analyses")
        self.log(
            f"Analyse terminee : {result.total_files} fichiers, {result.total_folders} dossiers, "
            f"{format_size(result.total_size)}"
        )
        if result.errors:
            self.log(f"{len(result.errors)} erreurs rencontrees (voir le fichier log).")

        self._refresh_all_tabs()

    def _refresh_all_tabs(self) -> None:
        self._refresh_summary_tab()
        self._refresh_junk_tab()
        self._refresh_big_files_tab()
        self._refresh_old_files_tab()
        self._refresh_empty_folders_tab()
        self._refresh_rare_extensions_tab()
        self._refresh_sort_tab()

    def _require_scan(self) -> bool:
        if self._scan_result is None:
            messagebox.showinfo("Aucune analyse", "Lancez d'abord une analyse de dossier.")
            return False
        return True

    # ------------------------------------------------------------------
    # Onglet Resume
    # ------------------------------------------------------------------

    def _build_summary_tab(self) -> None:
        frame = self._tab_summary
        self._summary_stats_label = ctk.CTkLabel(
            frame, text="Lancez une analyse pour voir le resume.", justify="left",
            font=ctk.CTkFont(size=14),
        )
        self._summary_stats_label.pack(anchor="w", padx=12, pady=12)

        columns_frame = ctk.CTkFrame(frame, fg_color="transparent")
        columns_frame.pack(fill="both", expand=True, padx=12, pady=8)
        columns_frame.grid_columnconfigure((0, 1), weight=1)
        columns_frame.grid_rowconfigure(0, weight=1)

        left = ctk.CTkFrame(columns_frame)
        left.grid(row=0, column=0, sticky="nsew", padx=(0, 8))
        ctk.CTkLabel(left, text="Repartition par extension", font=ctk.CTkFont(weight="bold")).pack(
            anchor="w", padx=8, pady=8
        )
        self._extension_box = ctk.CTkTextbox(left)
        self._extension_box.pack(fill="both", expand=True, padx=8, pady=(0, 8))

        right = ctk.CTkFrame(columns_frame)
        right.grid(row=0, column=1, sticky="nsew", padx=(8, 0))
        ctk.CTkLabel(right, text="Plus gros / plus anciens fichiers", font=ctk.CTkFont(weight="bold")).pack(
            anchor="w", padx=8, pady=8
        )
        self._top_files_box = ctk.CTkTextbox(right)
        self._top_files_box.pack(fill="both", expand=True, padx=8, pady=(0, 8))

    def _refresh_summary_tab(self) -> None:
        result = self._scan_result
        if result is None:
            return

        stats = (
            f"Dossier : {result.root}\n"
            f"Fichiers : {result.total_files}    Dossiers : {result.total_folders}    "
            f"Taille totale : {format_size(result.total_size)}"
        )
        self._summary_stats_label.configure(text=stats)

        self._extension_box.configure(state="normal")
        self._extension_box.delete("1.0", "end")
        for ext, (count, size) in sorted(
            result.extension_breakdown().items(), key=lambda kv: kv[1][1], reverse=True
        ):
            self._extension_box.insert("end", f"{ext or '(sans extension)'} : {count} fichiers - {format_size(size)}\n")
        self._extension_box.configure(state="disabled")

        self._top_files_box.configure(state="normal")
        self._top_files_box.delete("1.0", "end")
        self._top_files_box.insert("end", "== Plus gros fichiers ==\n")
        for f in result.largest_files(10):
            self._top_files_box.insert("end", f"{f.name} - {format_size(f.size)}\n")
        self._top_files_box.insert("end", "\n== Plus anciens fichiers ==\n")
        for f in result.oldest_files(10):
            self._top_files_box.insert("end", f"{f.name}\n")
        self._top_files_box.configure(state="disabled")

    # ------------------------------------------------------------------
    # Onglet Doublons
    # ------------------------------------------------------------------

    def _build_duplicates_tab(self) -> None:
        frame = self._tab_duplicates
        toolbar = ctk.CTkFrame(frame, fg_color="transparent")
        toolbar.pack(fill="x", padx=8, pady=8)

        ctk.CTkButton(toolbar, text="Detecter par contenu (hash)", command=self._detect_duplicates_content).pack(
            side="left", padx=4
        )
        ctk.CTkButton(toolbar, text="Detecter par nom", command=self._detect_duplicates_name).pack(
            side="left", padx=4
        )

        ctk.CTkLabel(toolbar, text="Conserver :").pack(side="left", padx=(20, 4))
        self._dup_strategy_var = ctk.StringVar(value="oldest")
        ctk.CTkOptionMenu(
            toolbar,
            variable=self._dup_strategy_var,
            values=["oldest", "newest", "largest", "smallest"],
        ).pack(side="left")

        ctk.CTkButton(
            toolbar, text="Appliquer la strategie (supprimer les autres)", command=self._apply_duplicate_strategy
        ).pack(side="left", padx=12)

        self._duplicates_summary = ctk.CTkLabel(frame, text="Aucune detection lancee.")
        self._duplicates_summary.pack(anchor="w", padx=8)

        self._duplicates_table = FileTable(
            frame,
            columns=(
                ("selected", "", 32),
                ("name", "Nom", 240),
                ("folder", "Dossier", 300),
                ("size", "Taille", 90),
                ("modified", "Modifie le", 130),
            ),
        )
        self._duplicates_table.pack(fill="both", expand=True, padx=8, pady=8)

        action_bar = ctk.CTkFrame(frame, fg_color="transparent")
        action_bar.pack(fill="x", padx=8, pady=(0, 8))
        ctk.CTkButton(action_bar, text="Tout cocher", command=lambda: self._duplicates_table.check_all(True)).pack(
            side="left", padx=4
        )
        ctk.CTkButton(action_bar, text="Tout decocher", command=lambda: self._duplicates_table.check_all(False)).pack(
            side="left", padx=4
        )
        ctk.CTkButton(
            action_bar, text="Supprimer la selection", fg_color="#c0392b", hover_color="#922b21",
            command=self._delete_selected_duplicates,
        ).pack(side="right", padx=4)

    def _detect_duplicates_content(self) -> None:
        if not self._require_scan():
            return
        self.log("Detection des doublons par contenu (hash SHA256)...")
        threading.Thread(target=self._run_duplicate_detection, args=(True,), daemon=True).start()

    def _detect_duplicates_name(self) -> None:
        if not self._require_scan():
            return
        self.log("Detection des doublons par nom...")
        threading.Thread(target=self._run_duplicate_detection, args=(False,), daemon=True).start()

    def _run_duplicate_detection(self, by_content: bool) -> None:
        assert self._scan_result is not None
        if by_content:
            groups = dup_analysis.find_duplicates_by_content(self._scan_result.files)
        else:
            groups = dup_analysis.find_duplicates_by_name(self._scan_result.files)
        self.after(0, self._on_duplicates_detected, groups)

    def _on_duplicates_detected(self, groups: list[dup_analysis.DuplicateGroup]) -> None:
        self._duplicate_groups = groups
        all_files = [f for g in groups for f in g.files]
        recoverable = sum(g.recoverable_size for g in groups)
        self._duplicates_summary.configure(
            text=f"{len(groups)} groupe(s) de doublons - {len(all_files)} fichiers - "
                 f"{format_size(recoverable)} recuperables"
        )
        self._duplicates_table.load(all_files, checked_by_default=False)
        self.log(f"{len(groups)} groupes de doublons detectes ({format_size(recoverable)} recuperables).")

    def _apply_duplicate_strategy(self) -> None:
        if not self._duplicate_groups:
            messagebox.showinfo("Aucun doublon", "Lancez d'abord une detection de doublons.")
            return
        strategy = self._dup_strategy_var.get()
        to_delete: list[FileInfo] = []
        for group in self._duplicate_groups:
            keep = group.suggest_original(strategy)
            to_delete.extend(f for f in group.files if f.path != keep.path)

        if not to_delete:
            messagebox.showinfo("Rien a faire", "Aucun fichier a supprimer avec cette strategie.")
            return
        self._confirm_and_delete(to_delete, label="Suppression de doublons")

    def _delete_selected_duplicates(self) -> None:
        selected = self._duplicates_table.checked_files()
        if not selected:
            messagebox.showinfo("Aucune selection", "Cochez au moins un fichier a supprimer.")
            return
        self._confirm_and_delete(selected, label="Suppression de doublons (selection manuelle)")

    # ------------------------------------------------------------------
    # Onglet Fichiers inutiles
    # ------------------------------------------------------------------

    def _build_junk_tab(self) -> None:
        frame = self._tab_junk
        self._junk_summary = ctk.CTkLabel(frame, text="Lancez une analyse pour detecter les fichiers inutiles.")
        self._junk_summary.pack(anchor="w", padx=8, pady=8)

        self._junk_table = FileTable(frame)
        self._junk_table.pack(fill="both", expand=True, padx=8, pady=8)

        action_bar = ctk.CTkFrame(frame, fg_color="transparent")
        action_bar.pack(fill="x", padx=8, pady=(0, 8))
        ctk.CTkButton(action_bar, text="Tout cocher", command=lambda: self._junk_table.check_all(True)).pack(
            side="left", padx=4
        )
        ctk.CTkButton(action_bar, text="Tout decocher", command=lambda: self._junk_table.check_all(False)).pack(
            side="left", padx=4
        )
        ctk.CTkButton(
            action_bar, text="Supprimer la selection", fg_color="#c0392b", hover_color="#922b21",
            command=self._delete_selected_junk,
        ).pack(side="right", padx=4)

    def _refresh_junk_tab(self) -> None:
        assert self._scan_result is not None
        junk_files = junk_analysis.find_junk_files(self._scan_result.files)
        total_size = sum(f.size for f in junk_files)
        self._junk_summary.configure(
            text=f"{len(junk_files)} fichier(s) inutile(s) detecte(s) - {format_size(total_size)}"
        )
        self._junk_table.load(junk_files, checked_by_default=True)

    def _delete_selected_junk(self) -> None:
        selected = self._junk_table.checked_files()
        if not selected:
            messagebox.showinfo("Aucune selection", "Cochez au moins un fichier a supprimer.")
            return
        self._confirm_and_delete(selected, label="Suppression de fichiers inutiles")

    # ------------------------------------------------------------------
    # Onglet Gros fichiers
    # ------------------------------------------------------------------

    def _build_big_files_tab(self) -> None:
        frame = self._tab_big
        toolbar = ctk.CTkFrame(frame, fg_color="transparent")
        toolbar.pack(fill="x", padx=8, pady=8)

        ctk.CTkLabel(toolbar, text="Seuil :").pack(side="left")
        self._big_filter_var = ctk.StringVar(value="100 Mo")
        ctk.CTkOptionMenu(
            toolbar, variable=self._big_filter_var,
            values=["100 Mo", "500 Mo", "1 Go", "Personnalise"],
            command=lambda _=None: self._refresh_big_files_tab(),
        ).pack(side="left", padx=6)

        self._big_custom_var = ctk.StringVar(value="200")
        ctk.CTkEntry(toolbar, textvariable=self._big_custom_var, width=80, placeholder_text="Mo").pack(
            side="left", padx=6
        )
        ctk.CTkButton(toolbar, text="Appliquer", command=self._refresh_big_files_tab).pack(side="left", padx=6)

        self._big_summary = ctk.CTkLabel(frame, text="")
        self._big_summary.pack(anchor="w", padx=8)

        self._big_table = FileTable(frame)
        self._big_table.pack(fill="both", expand=True, padx=8, pady=8)

        action_bar = ctk.CTkFrame(frame, fg_color="transparent")
        action_bar.pack(fill="x", padx=8, pady=(0, 8))
        ctk.CTkButton(
            action_bar, text="Supprimer la selection", fg_color="#c0392b", hover_color="#922b21",
            command=self._delete_selected_big,
        ).pack(side="right", padx=4)

    def _resolve_big_threshold_bytes(self) -> int:
        preset = self._big_filter_var.get()
        mb = 1024 * 1024
        if preset == "100 Mo":
            return 100 * mb
        if preset == "500 Mo":
            return 500 * mb
        if preset == "1 Go":
            return 1024 * mb
        try:
            return int(self._big_custom_var.get()) * mb
        except ValueError:
            return self._settings.big_file_threshold_mb * mb

    def _refresh_big_files_tab(self) -> None:
        if self._scan_result is None:
            return
        threshold = self._resolve_big_threshold_bytes()
        files = filter_analysis.find_big_files(self._scan_result.files, threshold)
        self._big_summary.configure(text=f"{len(files)} fichier(s) au-dessus de {format_size(threshold)}")
        self._big_table.load(files, checked_by_default=False)

    def _delete_selected_big(self) -> None:
        selected = self._big_table.checked_files()
        if not selected:
            messagebox.showinfo("Aucune selection", "Cochez au moins un fichier a supprimer.")
            return
        self._confirm_and_delete(selected, label="Suppression de gros fichiers")

    # ------------------------------------------------------------------
    # Onglet Anciens fichiers
    # ------------------------------------------------------------------

    def _build_old_files_tab(self) -> None:
        frame = self._tab_old
        toolbar = ctk.CTkFrame(frame, fg_color="transparent")
        toolbar.pack(fill="x", padx=8, pady=8)

        ctk.CTkLabel(toolbar, text="Anciennete :").pack(side="left")
        self._old_filter_var = ctk.StringVar(value="1 an")
        ctk.CTkOptionMenu(
            toolbar, variable=self._old_filter_var,
            values=["1 an", "2 ans", "5 ans", "Personnalise"],
            command=lambda _=None: self._refresh_old_files_tab(),
        ).pack(side="left", padx=6)

        self._old_custom_var = ctk.StringVar(value="90")
        ctk.CTkEntry(toolbar, textvariable=self._old_custom_var, width=80, placeholder_text="jours").pack(
            side="left", padx=6
        )
        ctk.CTkButton(toolbar, text="Appliquer", command=self._refresh_old_files_tab).pack(side="left", padx=6)

        self._old_summary = ctk.CTkLabel(frame, text="")
        self._old_summary.pack(anchor="w", padx=8)

        self._old_table = FileTable(frame)
        self._old_table.pack(fill="both", expand=True, padx=8, pady=8)

        action_bar = ctk.CTkFrame(frame, fg_color="transparent")
        action_bar.pack(fill="x", padx=8, pady=(0, 8))
        ctk.CTkButton(
            action_bar, text="Supprimer la selection", fg_color="#c0392b", hover_color="#922b21",
            command=self._delete_selected_old,
        ).pack(side="right", padx=4)

    def _resolve_old_threshold_days(self) -> int:
        preset = self._old_filter_var.get()
        if preset == "1 an":
            return 365
        if preset == "2 ans":
            return 730
        if preset == "5 ans":
            return 1825
        try:
            return int(self._old_custom_var.get())
        except ValueError:
            return self._settings.old_file_threshold_days

    def _refresh_old_files_tab(self) -> None:
        if self._scan_result is None:
            return
        min_days = self._resolve_old_threshold_days()
        files = filter_analysis.find_old_files(self._scan_result.files, min_days)
        self._old_summary.configure(text=f"{len(files)} fichier(s) non modifie(s) depuis plus de {min_days} jours")
        self._old_table.load(files, checked_by_default=False)

    def _delete_selected_old(self) -> None:
        selected = self._old_table.checked_files()
        if not selected:
            messagebox.showinfo("Aucune selection", "Cochez au moins un fichier a supprimer.")
            return
        self._confirm_and_delete(selected, label="Suppression de fichiers anciens")

    # ------------------------------------------------------------------
    # Onglet Dossiers vides
    # ------------------------------------------------------------------

    def _build_empty_folders_tab(self) -> None:
        frame = self._tab_empty
        toolbar = ctk.CTkFrame(frame, fg_color="transparent")
        toolbar.pack(fill="x", padx=8, pady=8)
        self._empty_summary = ctk.CTkLabel(toolbar, text="")
        self._empty_summary.pack(side="left")
        ctk.CTkButton(toolbar, text="Tout cocher", command=lambda: self._empty_list.check_all(True)).pack(
            side="left", padx=12
        )
        ctk.CTkButton(toolbar, text="Tout decocher", command=lambda: self._empty_list.check_all(False)).pack(
            side="left", padx=4
        )
        ctk.CTkButton(
            toolbar, text="Supprimer les dossiers coches", fg_color="#c0392b", hover_color="#922b21",
            command=self._delete_selected_empty_folders,
        ).pack(side="right", padx=4)

        self._empty_list = FolderList(frame)
        self._empty_list.pack(fill="both", expand=True, padx=8, pady=8)

    def _refresh_empty_folders_tab(self) -> None:
        assert self._scan_result is not None
        empty_folders = filter_analysis.find_empty_folders(self._scan_result.folders)
        self._empty_summary.configure(text=f"{len(empty_folders)} dossier(s) vide(s) detecte(s)")
        self._empty_list.load(empty_folders)

    def _delete_selected_empty_folders(self) -> None:
        selected = self._empty_list.checked_paths()
        if not selected:
            messagebox.showinfo("Aucune selection", "Cochez au moins un dossier a supprimer.")
            return
        if self._settings.ask_confirmation and not messagebox.askyesno(
            "Confirmer", f"Supprimer {len(selected)} dossier(s) vide(s) ?"
        ):
            return
        report = self._action_engine.delete_empty_folders(selected, self._current_folder)
        self.log(f"{report.succeeded} dossier(s) vide(s) supprime(s), {len(report.failed)} echec(s).")
        self._refresh_empty_folders_tab()
        self._refresh_history_tab()

    # ------------------------------------------------------------------
    # Onglet Extensions rares
    # ------------------------------------------------------------------

    def _build_rare_extensions_tab(self) -> None:
        frame = self._tab_rare
        toolbar = ctk.CTkFrame(frame, fg_color="transparent")
        toolbar.pack(fill="x", padx=8, pady=8)
        ctk.CTkLabel(toolbar, text="Occurrences maximum :").pack(side="left")
        self._rare_threshold_var = ctk.StringVar(value="3")
        ctk.CTkEntry(toolbar, textvariable=self._rare_threshold_var, width=60).pack(side="left", padx=6)
        ctk.CTkButton(toolbar, text="Appliquer", command=self._refresh_rare_extensions_tab).pack(side="left", padx=6)

        self._rare_table = FileTable(
            frame,
            columns=(
                ("selected", "", 32),
                ("name", "Nom", 240),
                ("extension", "Extension", 100),
                ("folder", "Dossier", 300),
                ("size", "Taille", 90),
            ),
        )
        self._rare_table.pack(fill="both", expand=True, padx=8, pady=8)

    def _refresh_rare_extensions_tab(self) -> None:
        if self._scan_result is None:
            return
        try:
            threshold = int(self._rare_threshold_var.get())
        except ValueError:
            threshold = 3
        grouped = filter_analysis.find_rare_extensions(self._scan_result.files, threshold)
        flat = [f for files in grouped.values() for f in files]
        self._rare_table.load(flat, checked_by_default=False)

    # ------------------------------------------------------------------
    # Onglet Classement et renommage
    # ------------------------------------------------------------------

    def _build_sort_rename_tab(self) -> None:
        frame = self._tab_sort

        sort_section = ctk.CTkFrame(frame)
        sort_section.pack(fill="x", padx=8, pady=8)
        ctk.CTkLabel(sort_section, text="Classement automatique par categorie", font=ctk.CTkFont(weight="bold")).pack(
            anchor="w", padx=8, pady=(8, 4)
        )
        self._sort_summary = ctk.CTkLabel(sort_section, text="Lancez une analyse pour voir les categories detectees.")
        self._sort_summary.pack(anchor="w", padx=8)

        self._category_scroll = ctk.CTkScrollableFrame(sort_section, height=160)
        self._category_scroll.pack(fill="x", padx=8, pady=8)
        self._category_vars: dict[str, ctk.BooleanVar] = {}

        ctk.CTkButton(sort_section, text="Classer les fichiers selectionnes", command=self._apply_classification).pack(
            anchor="w", padx=8, pady=(0, 8)
        )

        rename_section = ctk.CTkFrame(frame)
        rename_section.pack(fill="both", expand=True, padx=8, pady=8)
        ctk.CTkLabel(rename_section, text="Renommage intelligent", font=ctk.CTkFont(weight="bold")).pack(
            anchor="w", padx=8, pady=(8, 4)
        )
        ctk.CTkLabel(
            rename_section,
            text="Choisissez les fichiers a renommer (par categorie), donnez un libelle optionnel, "
                 "puis previsualisez avant d'appliquer.",
            wraplength=800, justify="left",
        ).pack(anchor="w", padx=8)

        options_row = ctk.CTkFrame(rename_section, fg_color="transparent")
        options_row.pack(fill="x", padx=8, pady=8)
        ctk.CTkLabel(options_row, text="Categorie source :").pack(side="left")
        self._rename_category_var = ctk.StringVar(value="Toutes")
        self._rename_category_menu = ctk.CTkOptionMenu(options_row, variable=self._rename_category_var, values=["Toutes"])
        self._rename_category_menu.pack(side="left", padx=6)

        ctk.CTkLabel(options_row, text="Libelle (optionnel) :").pack(side="left", padx=(16, 4))
        self._rename_label_var = ctk.StringVar(value="")
        ctk.CTkEntry(options_row, textvariable=self._rename_label_var, placeholder_text="ex: Vacances_Paris").pack(
            side="left", padx=4
        )

        self._rename_date_var = ctk.BooleanVar(value=True)
        ctk.CTkCheckBox(
            options_row, text="Utiliser la date detectee si pas de libelle", variable=self._rename_date_var
        ).pack(side="left", padx=16)

        ctk.CTkButton(rename_section, text="Previsualiser le renommage", command=self._preview_rename).pack(
            anchor="w", padx=8, pady=8
        )

    def _refresh_sort_tab(self) -> None:
        assert self._scan_result is not None
        for widget in self._category_scroll.winfo_children():
            widget.destroy()
        self._category_vars.clear()

        breakdown = self._scan_result.category_breakdown()
        for category in self._categorizer.category_names():
            count, size = breakdown.get(category, (0, 0))
            var = ctk.BooleanVar(value=count > 0)
            self._category_vars[category] = var
            ctk.CTkCheckBox(
                self._category_scroll, text=f"{category}  ({count} fichiers - {format_size(size)})", variable=var
            ).pack(anchor="w", padx=4, pady=2)

        self._sort_summary.configure(text=f"{len(breakdown)} categorie(s) presente(s) dans le dossier analyse.")
        self._rename_category_menu.configure(values=["Toutes"] + self._categorizer.category_names())

    def _apply_classification(self) -> None:
        if not self._require_scan():
            return
        selected_categories = {cat for cat, var in self._category_vars.items() if var.get()}
        if not selected_categories:
            messagebox.showinfo("Aucune categorie", "Selectionnez au moins une categorie a classer.")
            return

        mapping: list[tuple[FileInfo, Path]] = []
        for f in self._scan_result.files:
            if f.category in selected_categories:
                dest_folder = self._current_folder / f.category
                if f.path.parent != dest_folder:
                    mapping.append((f, dest_folder))

        if not mapping:
            messagebox.showinfo("Rien a faire", "Tous les fichiers de ces categories sont deja bien ranges.")
            return

        if self._settings.ask_confirmation and not messagebox.askyesno(
            "Confirmer le classement",
            f"Deplacer {len(mapping)} fichier(s) vers des sous-dossiers par categorie ?",
        ):
            return

        report = self._action_engine.move_files(mapping, self._current_folder)
        self.log(f"Classement : {report.succeeded} fichier(s) deplace(s), {len(report.failed)} echec(s).")
        self._refresh_history_tab()

    def _preview_rename(self) -> None:
        if not self._require_scan():
            return
        category = self._rename_category_var.get()
        files = self._scan_result.files if category == "Toutes" else [
            f for f in self._scan_result.files if f.category == category
        ]
        if not files:
            messagebox.showinfo("Aucun fichier", "Aucun fichier ne correspond a ce filtre.")
            return

        label = self._rename_label_var.get().strip() or None
        plan = build_rename_plan(files, label=label, use_date_prefix=self._rename_date_var.get())

        def on_apply(final_plan) -> None:
            report = self._action_engine.apply_rename_plan(final_plan, self._current_folder)
            self.log(f"Renommage : {report.succeeded} fichier(s) renomme(s), {len(report.failed)} echec(s).")
            self._refresh_history_tab()

        RenamePreviewDialog(self, plan, on_apply)

    # ------------------------------------------------------------------
    # Onglet Recherche
    # ------------------------------------------------------------------

    def _build_search_tab(self) -> None:
        frame = self._tab_search
        form = ctk.CTkFrame(frame, fg_color="transparent")
        form.pack(fill="x", padx=8, pady=8)

        ctk.CTkLabel(form, text="Nom contient :").grid(row=0, column=0, sticky="w", padx=4, pady=4)
        self._search_text_var = ctk.StringVar()
        ctk.CTkEntry(form, textvariable=self._search_text_var, width=200).grid(row=0, column=1, padx=4, pady=4)

        ctk.CTkLabel(form, text="Extensions (ex: .jpg,.png) :").grid(row=0, column=2, sticky="w", padx=4, pady=4)
        self._search_ext_var = ctk.StringVar()
        ctk.CTkEntry(form, textvariable=self._search_ext_var, width=180).grid(row=0, column=3, padx=4, pady=4)

        ctk.CTkLabel(form, text="Dossier contient :").grid(row=1, column=0, sticky="w", padx=4, pady=4)
        self._search_folder_var = ctk.StringVar()
        ctk.CTkEntry(form, textvariable=self._search_folder_var, width=200).grid(row=1, column=1, padx=4, pady=4)

        ctk.CTkLabel(form, text="Taille min (Mo) :").grid(row=1, column=2, sticky="w", padx=4, pady=4)
        self._search_min_size_var = ctk.StringVar()
        ctk.CTkEntry(form, textvariable=self._search_min_size_var, width=100).grid(
            row=1, column=3, padx=4, pady=4, sticky="w"
        )

        ctk.CTkLabel(form, text="Type :").grid(row=2, column=0, sticky="w", padx=4, pady=4)
        self._search_category_var = ctk.StringVar(value="Toutes")
        self._search_category_menu = ctk.CTkOptionMenu(form, variable=self._search_category_var, values=["Toutes"])
        self._search_category_menu.grid(row=2, column=1, padx=4, pady=4, sticky="w")

        ctk.CTkButton(form, text="Rechercher", command=self._run_search).grid(row=2, column=3, padx=4, pady=4, sticky="e")

        self._search_summary = ctk.CTkLabel(frame, text="")
        self._search_summary.pack(anchor="w", padx=8)

        self._search_table = FileTable(frame)
        self._search_table.pack(fill="both", expand=True, padx=8, pady=8)

        action_bar = ctk.CTkFrame(frame, fg_color="transparent")
        action_bar.pack(fill="x", padx=8, pady=(0, 8))
        ctk.CTkButton(
            action_bar, text="Supprimer la selection", fg_color="#c0392b", hover_color="#922b21",
            command=self._delete_selected_search,
        ).pack(side="right", padx=4)

    def _run_search(self) -> None:
        if not self._require_scan():
            return
        extensions = [e.strip() for e in self._search_ext_var.get().split(",") if e.strip()] or None
        category = self._search_category_var.get()
        categories = None if category == "Toutes" else [category]
        try:
            min_size = int(self._search_min_size_var.get()) * 1024 * 1024 if self._search_min_size_var.get() else None
        except ValueError:
            min_size = None

        criteria = SearchCriteria(
            text=self._search_text_var.get().strip() or None,
            extensions=extensions,
            categories=categories,
            min_size_bytes=min_size,
            folder=self._search_folder_var.get().strip() or None,
        )
        results = search_files(self._scan_result.files, criteria)
        self._search_summary.configure(text=f"{len(results)} resultat(s)")
        self._search_table.load(results, checked_by_default=False)
        self._search_category_menu.configure(values=["Toutes"] + self._categorizer.category_names())

    def _delete_selected_search(self) -> None:
        selected = self._search_table.checked_files()
        if not selected:
            messagebox.showinfo("Aucune selection", "Cochez au moins un fichier a supprimer.")
            return
        self._confirm_and_delete(selected, label="Suppression depuis la recherche")

    # ------------------------------------------------------------------
    # Onglet Historique
    # ------------------------------------------------------------------

    def _build_history_tab(self) -> None:
        frame = self._tab_history
        ctk.CTkButton(frame, text="Rafraichir", command=self._refresh_history_tab).pack(anchor="e", padx=8, pady=8)
        self._history_scroll = ctk.CTkScrollableFrame(frame)
        self._history_scroll.pack(fill="both", expand=True, padx=8, pady=8)
        self._refresh_history_tab()

    def _refresh_history_tab(self) -> None:
        for widget in self._history_scroll.winfo_children():
            widget.destroy()

        sessions = self._history.all_sessions()
        if not sessions:
            ctk.CTkLabel(self._history_scroll, text="Aucune operation enregistree pour le moment.").pack(
                anchor="w", padx=8, pady=8
            )
            return

        for session in sessions:
            row = ctk.CTkFrame(self._history_scroll)
            row.pack(fill="x", padx=4, pady=4)

            text = (
                f"{session.timestamp}  -  {session.label}\n"
                f"Dossier : {session.scanned_folder}\n"
                f"Supprimes : {session.files_deleted}   Deplaces : {session.files_moved}   "
                f"Renommes : {session.files_renamed}"
            )
            ctk.CTkLabel(row, text=text, justify="left", anchor="w").pack(
                side="left", padx=8, pady=8, fill="x", expand=True
            )
            ctk.CTkButton(
                row, text="Restaurer", width=100, command=lambda sid=session.id: self._restore_session(sid)
            ).pack(side="right", padx=8)

    def _restore_session(self, session_id: str) -> None:
        if not messagebox.askyesno("Confirmer", "Restaurer les operations reversibles de cette session ?"):
            return
        restored, failed = self._history.restore_session(session_id)
        self.log(f"Restauration : {restored} operation(s) restauree(s), {failed} echec(s).")
        self._refresh_history_tab()

    # ------------------------------------------------------------------
    # Suppression avec confirmation (partagee entre tous les onglets)
    # ------------------------------------------------------------------

    def _confirm_and_delete(self, files: list[FileInfo], label: str) -> None:
        total_size = sum(f.size for f in files)
        destination = "Corbeille systeme" if self._settings.use_recycle_bin else "Dossier de quarantaine local"

        def do_delete() -> None:
            report = self._action_engine.delete_files(files, self._current_folder, label=label)
            self.log(
                f"{label} : {report.succeeded} fichier(s) supprime(s), "
                f"{format_size(report.freed_bytes)} liberes, {len(report.failed)} echec(s)."
            )
            self._refresh_all_tabs()
            self._refresh_history_tab()

        if self._settings.ask_confirmation:
            ConfirmDeleteDialog(self, len(files), total_size, destination, on_confirm=do_delete)
        else:
            do_delete()

    # ------------------------------------------------------------------
    # Export
    # ------------------------------------------------------------------

    def _export(self, fmt: str) -> None:
        if not self._require_scan():
            return
        default_name = exporter.default_export_filename(fmt, self._scan_result.root)
        path_str = filedialog.asksaveasfilename(
            defaultextension=f".{fmt}", initialfile=default_name,
            filetypes=[(fmt.upper(), f"*.{fmt}")],
        )
        if not path_str:
            return
        output_path = Path(path_str)
        try:
            if fmt == "csv":
                exporter.export_csv(self._scan_result, output_path)
            elif fmt == "json":
                exporter.export_json(self._scan_result, output_path)
            elif fmt == "pdf":
                exporter.export_pdf(self._scan_result, output_path)
            self.log(f"Export {fmt.upper()} genere : {output_path}")
        except OSError as exc:
            messagebox.showerror("Erreur d'export", str(exc))

    # ------------------------------------------------------------------
    # Parametres
    # ------------------------------------------------------------------

    def _open_settings(self) -> None:
        def on_saved(settings: Settings) -> None:
            self._settings = settings
            ctk.set_appearance_mode(settings.theme)
            configure_treeview_style(settings.theme if settings.theme != "system" else "dark")
            self._categorizer.refresh(settings)
            self.log("Parametres enregistres.")

        SettingsDialog(self, self._settings, on_saved)
