"""
Widgets reutilisables pour l'interface File Cleaner.
"""
from __future__ import annotations

import tkinter as tk
from collections.abc import Callable, Iterable
from tkinter import ttk

import customtkinter as ctk

from core.models import FileInfo
from utils.helpers import format_datetime, format_size


def configure_treeview_style(mode: str = "dark") -> None:
    """Applique un style sombre/clair coherent a ttk.Treeview (non gere par CTk)."""
    style = ttk.Style()
    style.theme_use("clam")

    if mode == "dark":
        bg, fg, field_bg, selected = "#2b2b2b", "#e6e6e6", "#242424", "#1f6aa5"
        header_bg = "#333333"
    else:
        bg, fg, field_bg, selected = "#ffffff", "#1a1a1a", "#f5f5f5", "#3b8ed0"
        header_bg = "#e5e5e5"

    style.configure(
        "FileCleaner.Treeview",
        background=field_bg,
        foreground=fg,
        fieldbackground=field_bg,
        borderwidth=0,
        rowheight=26,
    )
    style.map("FileCleaner.Treeview", background=[("selected", selected)])
    style.configure(
        "FileCleaner.Treeview.Heading",
        background=header_bg,
        foreground=fg,
        borderwidth=0,
        relief="flat",
    )
    style.map("FileCleaner.Treeview.Heading", background=[("active", header_bg)])


class FileTable(ctk.CTkFrame):
    """
    Tableau de fichiers avec cases a cocher (selection multiple), tri par
    colonne et formatage automatique des tailles/dates. Base sur
    ttk.Treeview pour des performances correctes avec de gros volumes.
    """

    def __init__(
        self,
        master,
        columns: Iterable[tuple[str, str, int]] = (
            ("selected", "", 32),
            ("name", "Nom", 260),
            ("folder", "Dossier", 320),
            ("size", "Taille", 100),
            ("modified", "Modifie le", 140),
        ),
        on_selection_change: Callable[[list[FileInfo]], None] | None = None,
        **kwargs,
    ):
        super().__init__(master, fg_color="transparent", **kwargs)
        self._columns = list(columns)
        self._on_selection_change = on_selection_change
        self._checked: dict[str, bool] = {}
        self._row_to_file: dict[str, FileInfo] = {}

        col_ids = [c[0] for c in self._columns]
        self.tree = ttk.Treeview(
            self,
            columns=col_ids,
            show="headings",
            style="FileCleaner.Treeview",
            selectmode="extended",
        )
        for col_id, label, width in self._columns:
            self.tree.heading(col_id, text=label, command=lambda c=col_id: self._sort_by(c))
            anchor = "center" if col_id == "selected" else "w"
            self.tree.column(col_id, width=width, anchor=anchor, stretch=(col_id != "selected"))

        vsb = ttk.Scrollbar(self, orient="vertical", command=self.tree.yview)
        hsb = ttk.Scrollbar(self, orient="horizontal", command=self.tree.xview)
        self.tree.configure(yscrollcommand=vsb.set, xscrollcommand=hsb.set)

        self.tree.grid(row=0, column=0, sticky="nsew")
        vsb.grid(row=0, column=1, sticky="ns")
        hsb.grid(row=1, column=0, sticky="ew")
        self.grid_rowconfigure(0, weight=1)
        self.grid_columnconfigure(0, weight=1)

        self.tree.bind("<Button-1>", self._on_click)
        self._sort_state: dict[str, bool] = {}

    # ------------------------------------------------------------------

    def load(self, files: list[FileInfo], checked_by_default: bool = True) -> None:
        self.tree.delete(*self.tree.get_children())
        self._checked.clear()
        self._row_to_file.clear()

        for file in files:
            row_id = str(file.path)
            values = self._row_values(file, checked_by_default)
            self.tree.insert("", "end", iid=row_id, values=values)
            self._checked[row_id] = checked_by_default
            self._row_to_file[row_id] = file

        self._notify_selection_change()

    def _row_values(self, file: FileInfo, checked: bool) -> list[str]:
        mapping = {
            "selected": "\u2611" if checked else "\u2610",
            "name": file.name,
            "folder": str(file.path.parent),
            "size": format_size(file.size),
            "modified": format_datetime(file.modified),
            "category": file.category,
            "extension": file.extension,
            "created": format_datetime(file.created),
            "accessed": format_datetime(file.accessed),
        }
        return [mapping.get(col_id, "") for col_id, _, _ in self._columns]

    def _on_click(self, event: tk.Event) -> None:
        region = self.tree.identify("region", event.x, event.y)
        if region != "cell":
            return
        column = self.tree.identify_column(event.x)
        row_id = self.tree.identify_row(event.y)
        if not row_id:
            return
        col_index = int(column.replace("#", "")) - 1
        if 0 <= col_index < len(self._columns) and self._columns[col_index][0] == "selected":
            self.toggle(row_id)

    def toggle(self, row_id: str) -> None:
        self._checked[row_id] = not self._checked.get(row_id, False)
        values = list(self.tree.item(row_id, "values"))
        values[0] = "\u2611" if self._checked[row_id] else "\u2610"
        self.tree.item(row_id, values=values)
        self._notify_selection_change()

    def check_all(self, checked: bool = True) -> None:
        for row_id in self.tree.get_children():
            self._checked[row_id] = checked
            values = list(self.tree.item(row_id, "values"))
            values[0] = "\u2611" if checked else "\u2610"
            self.tree.item(row_id, values=values)
        self._notify_selection_change()

    def checked_files(self) -> list[FileInfo]:
        return [self._row_to_file[rid] for rid, checked in self._checked.items() if checked]

    def all_files(self) -> list[FileInfo]:
        return list(self._row_to_file.values())

    def _notify_selection_change(self) -> None:
        if self._on_selection_change:
            self._on_selection_change(self.checked_files())

    def _sort_by(self, col_id: str) -> None:
        reverse = self._sort_state.get(col_id, False)
        items = [(self.tree.set(k, col_id), k) for k in self.tree.get_children("")]

        def sort_key(pair: tuple[str, str]):
            text = pair[0]
            cleaned = text.replace(" ", "").replace(",", ".")
            for unit in ("Po", "To", "Go", "Mo", "Ko", "o"):
                cleaned = cleaned.replace(unit, "")
            try:
                return float(cleaned)
            except ValueError:
                return text.lower()

        items.sort(key=sort_key, reverse=reverse)
        for index, (_, k) in enumerate(items):
            self.tree.move(k, "", index)
        self._sort_state[col_id] = not reverse

    def summary_label(self) -> str:
        n_checked = len(self.checked_files())
        n_total = len(self._row_to_file)
        size = sum(f.size for f in self.checked_files())
        return f"{n_checked}/{n_total} fichiers selectionnes - {format_size(size)}"


class FolderList(ctk.CTkScrollableFrame):
    """Liste de dossiers avec cases a cocher, utilisee pour les dossiers vides."""

    def __init__(self, master, **kwargs):
        super().__init__(master, **kwargs)
        self._vars: dict[str, ctk.BooleanVar] = {}
        self._paths: dict[str, object] = {}

    def load(self, folders: Iterable) -> None:
        for widget in self.winfo_children():
            widget.destroy()
        self._vars.clear()
        self._paths.clear()

        for folder in folders:
            key = str(folder)
            var = ctk.BooleanVar(value=True)
            self._vars[key] = var
            self._paths[key] = folder
            ctk.CTkCheckBox(self, text=key, variable=var).pack(anchor="w", pady=2, padx=4)

    def check_all(self, checked: bool = True) -> None:
        for var in self._vars.values():
            var.set(checked)

    def checked_paths(self) -> list:
        return [self._paths[k] for k, v in self._vars.items() if v.get()]

    def count(self) -> int:
        return len(self._vars)


class DropZone(ctk.CTkFrame):
    """Zone de glisser-deposer (avec repli gracieux si tkinterdnd2 est absent)."""

    def __init__(self, master, on_folder_dropped: Callable[[str], None], **kwargs):
        super().__init__(master, corner_radius=12, border_width=2, border_color=("gray70", "gray30"), **kwargs)
        self._on_folder_dropped = on_folder_dropped

        self.label = ctk.CTkLabel(
            self,
            text="\U0001F4C1  Glissez-deposez un dossier ici",
            font=ctk.CTkFont(size=15, weight="bold"),
        )
        self.label.pack(expand=True, fill="both", padx=20, pady=30)

        self._dnd_enabled = False
        self._try_enable_dnd()

    def _try_enable_dnd(self) -> None:
        try:
            from tkinterdnd2 import DND_FILES

            self.drop_target_register(DND_FILES)  # type: ignore[attr-defined]
            self.dnd_bind("<<Drop>>", self._handle_drop)  # type: ignore[attr-defined]
            self._dnd_enabled = True
        except Exception:
            self.label.configure(
                text="\U0001F4C1  Glisser-deposer indisponible\nUtilisez le bouton \u00ab Choisir un dossier \u00bb"
            )

    def _handle_drop(self, event) -> None:  # pragma: no cover - depend de l'OS
        raw = event.data.strip()
        if raw.startswith("{") and raw.endswith("}"):
            raw = raw[1:-1]
        path = raw.split("} {")[0] if "} {" in raw else raw
        self._on_folder_dropped(path)
