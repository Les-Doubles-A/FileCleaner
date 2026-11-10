"""
Boites de dialogue : confirmation avant suppression et previsualisation
du renommage intelligent (editable avant validation).
"""
from __future__ import annotations

from collections.abc import Callable

import customtkinter as ctk

from cleaner.rename_engine import RenamePlanItem
from utils.helpers import format_size


class ConfirmDeleteDialog(ctk.CTkToplevel):
    """Demande de confirmation avant toute suppression, avec resume clair."""

    def __init__(
        self,
        master,
        n_files: int,
        total_size: int,
        destination_hint: str,
        on_confirm: Callable[[], None],
    ):
        super().__init__(master)
        self.title("Confirmation requise")
        self.geometry("460x260")
        self.resizable(False, False)
        self.transient(master)
        self.grab_set()

        ctk.CTkLabel(
            self,
            text="\u26A0  Confirmer l'operation",
            font=ctk.CTkFont(size=18, weight="bold"),
        ).pack(pady=(20, 8))

        summary = (
            f"{n_files} fichier(s) concerne(s)\n"
            f"Espace liberable : {format_size(total_size)}\n\n"
            f"Destination : {destination_hint}\n\n"
            "Cette action peut etre annulee depuis l'historique."
        )
        ctk.CTkLabel(self, text=summary, justify="left").pack(padx=24, pady=8, fill="x")

        button_bar = ctk.CTkFrame(self, fg_color="transparent")
        button_bar.pack(side="bottom", pady=20)

        def _confirm() -> None:
            self.destroy()
            on_confirm()

        ctk.CTkButton(button_bar, text="Annuler", fg_color="gray40", command=self.destroy).pack(
            side="left", padx=8
        )
        ctk.CTkButton(button_bar, text="Confirmer", fg_color="#c0392b", hover_color="#922b21", command=_confirm).pack(
            side="left", padx=8
        )


class RenamePreviewDialog(ctk.CTkToplevel):
    """Previsualisation editable du plan de renommage avant application."""

    def __init__(self, master, plan: list[RenamePlanItem], on_apply: Callable[[list[RenamePlanItem]], None]):
        super().__init__(master)
        self.title("Previsualisation du renommage")
        self.geometry("760x560")
        self.transient(master)
        self.grab_set()

        self._plan = plan
        self._on_apply_callback = on_apply
        self._entries: list[tuple[RenamePlanItem, ctk.BooleanVar, ctk.StringVar]] = []

        ctk.CTkLabel(
            self,
            text="Verifiez et modifiez les nouveaux noms avant de valider.",
            font=ctk.CTkFont(weight="bold"),
        ).pack(anchor="w", padx=16, pady=(16, 8))

        scroll = ctk.CTkScrollableFrame(self)
        scroll.pack(fill="both", expand=True, padx=16, pady=8)

        header = ctk.CTkFrame(scroll, fg_color="transparent")
        header.pack(fill="x", pady=(0, 4))
        ctk.CTkLabel(header, text="", width=30).pack(side="left")
        ctk.CTkLabel(header, text="Nom actuel", width=260, anchor="w").pack(side="left", padx=4)
        ctk.CTkLabel(header, text="Nouveau nom", width=260, anchor="w").pack(side="left", padx=4)

        for item in plan:
            row = ctk.CTkFrame(scroll, fg_color="transparent")
            row.pack(fill="x", pady=2)

            enabled_var = ctk.BooleanVar(value=item.enabled)
            ctk.CTkCheckBox(row, text="", variable=enabled_var, width=30).pack(side="left")

            ctk.CTkLabel(row, text=item.file.name, width=260, anchor="w").pack(side="left", padx=4)

            name_var = ctk.StringVar(value=item.new_name)
            ctk.CTkEntry(row, textvariable=name_var, width=260).pack(side="left", padx=4)

            self._entries.append((item, enabled_var, name_var))

        button_bar = ctk.CTkFrame(self, fg_color="transparent")
        button_bar.pack(fill="x", padx=16, pady=16)
        ctk.CTkButton(button_bar, text="Annuler", fg_color="gray40", command=self.destroy).pack(
            side="right", padx=(8, 0)
        )
        ctk.CTkButton(button_bar, text="Appliquer le renommage", command=self._apply).pack(side="right")

    def _apply(self) -> None:
        final_plan = []
        for item, enabled_var, name_var in self._entries:
            item.enabled = enabled_var.get()
            item.new_name = name_var.get().strip() or item.new_name
            final_plan.append(item)
        self.destroy()
        self._on_apply_callback(final_plan)
