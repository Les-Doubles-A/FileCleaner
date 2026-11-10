"""
Fenetre modale des parametres (dossiers exclus, extensions ignorees,
tailles min/max, theme, langue, confirmations, categories).
"""
from __future__ import annotations

from collections.abc import Callable
from tkinter import filedialog

import customtkinter as ctk

from utils.config import DEFAULT_CATEGORIES, Settings, save_settings


class SettingsDialog(ctk.CTkToplevel):
    def __init__(self, master, settings: Settings, on_saved: Callable[[Settings], None]):
        super().__init__(master)
        self.title("Parametres - File Cleaner")
        self.geometry("640x620")
        self.minsize(560, 480)
        self.transient(master)
        self.grab_set()

        self._settings = settings
        self._on_saved = on_saved

        tabs = ctk.CTkTabview(self)
        tabs.pack(fill="both", expand=True, padx=16, pady=16)

        self._tab_general = tabs.add("General")
        self._tab_filters = tabs.add("Filtres")
        self._tab_categories = tabs.add("Categories")

        self._build_general_tab()
        self._build_filters_tab()
        self._build_categories_tab()

        button_bar = ctk.CTkFrame(self, fg_color="transparent")
        button_bar.pack(fill="x", padx=16, pady=(0, 16))
        ctk.CTkButton(button_bar, text="Annuler", fg_color="gray40", command=self.destroy).pack(
            side="right", padx=(8, 0)
        )
        ctk.CTkButton(button_bar, text="Enregistrer", command=self._save).pack(side="right")

    # ------------------------------------------------------------------
    # Onglet General
    # ------------------------------------------------------------------

    def _build_general_tab(self) -> None:
        frame = self._tab_general

        ctk.CTkLabel(frame, text="Theme", font=ctk.CTkFont(weight="bold")).pack(
            anchor="w", padx=8, pady=(12, 0)
        )
        self._theme_var = ctk.StringVar(value=self._settings.theme)
        ctk.CTkSegmentedButton(
            frame, values=["dark", "light", "system"], variable=self._theme_var
        ).pack(anchor="w", padx=8, pady=6)

        ctk.CTkLabel(frame, text="Langue", font=ctk.CTkFont(weight="bold")).pack(
            anchor="w", padx=8, pady=(12, 0)
        )
        self._lang_var = ctk.StringVar(value=self._settings.language)
        ctk.CTkSegmentedButton(frame, values=["fr", "en"], variable=self._lang_var).pack(
            anchor="w", padx=8, pady=6
        )

        self._confirm_var = ctk.BooleanVar(value=self._settings.ask_confirmation)
        ctk.CTkCheckBox(
            frame, text="Demander confirmation avant toute suppression", variable=self._confirm_var
        ).pack(anchor="w", padx=8, pady=(16, 4))

        self._recycle_var = ctk.BooleanVar(value=self._settings.use_recycle_bin)
        ctk.CTkCheckBox(
            frame,
            text="Utiliser la corbeille systeme (sinon : dossier de quarantaine)",
            variable=self._recycle_var,
        ).pack(anchor="w", padx=8, pady=4)

        ctk.CTkLabel(frame, text="Dossier de quarantaine personnalise (optionnel)").pack(
            anchor="w", padx=8, pady=(16, 0)
        )
        row = ctk.CTkFrame(frame, fg_color="transparent")
        row.pack(fill="x", padx=8, pady=6)
        self._quarantine_var = ctk.StringVar(value=self._settings.quarantine_folder or "")
        ctk.CTkEntry(row, textvariable=self._quarantine_var).pack(side="left", fill="x", expand=True)
        ctk.CTkButton(row, text="Parcourir", width=90, command=self._browse_quarantine).pack(
            side="left", padx=(6, 0)
        )

        ctk.CTkLabel(frame, text="Threads pour le scan").pack(anchor="w", padx=8, pady=(16, 0))
        self._workers_var = ctk.IntVar(value=self._settings.max_workers)
        ctk.CTkSlider(frame, from_=1, to=32, number_of_steps=31, variable=self._workers_var).pack(
            fill="x", padx=8, pady=6
        )

    def _browse_quarantine(self) -> None:
        folder = filedialog.askdirectory()
        if folder:
            self._quarantine_var.set(folder)

    # ------------------------------------------------------------------
    # Onglet Filtres
    # ------------------------------------------------------------------

    def _build_filters_tab(self) -> None:
        frame = self._tab_filters

        ctk.CTkLabel(frame, text="Dossiers exclus (un par ligne)", font=ctk.CTkFont(weight="bold")).pack(
            anchor="w", padx=8, pady=(12, 0)
        )
        self._excluded_box = ctk.CTkTextbox(frame, height=90)
        self._excluded_box.pack(fill="x", padx=8, pady=6)
        self._excluded_box.insert("1.0", "\n".join(self._settings.excluded_folders))

        add_row = ctk.CTkFrame(frame, fg_color="transparent")
        add_row.pack(fill="x", padx=8)
        ctk.CTkButton(add_row, text="+ Ajouter un dossier exclu", command=self._add_excluded_folder).pack(
            side="left"
        )

        ctk.CTkLabel(
            frame, text="Extensions ignorees (separees par des virgules)", font=ctk.CTkFont(weight="bold")
        ).pack(anchor="w", padx=8, pady=(16, 0))
        self._ignored_ext_var = ctk.StringVar(value=", ".join(self._settings.ignored_extensions))
        ctk.CTkEntry(frame, textvariable=self._ignored_ext_var, placeholder_text=".tmp, .log").pack(
            fill="x", padx=8, pady=6
        )

        size_frame = ctk.CTkFrame(frame, fg_color="transparent")
        size_frame.pack(fill="x", padx=8, pady=(16, 0))
        ctk.CTkLabel(size_frame, text="Taille min (Mo)").grid(row=0, column=0, sticky="w")
        self._min_size_var = ctk.StringVar(
            value=str((self._settings.min_size_bytes or 0) // (1024 * 1024))
        )
        ctk.CTkEntry(size_frame, textvariable=self._min_size_var, width=100).grid(
            row=0, column=1, padx=8
        )
        ctk.CTkLabel(size_frame, text="Taille max (Mo, 0 = illimite)").grid(row=1, column=0, sticky="w", pady=6)
        self._max_size_var = ctk.StringVar(
            value=str((self._settings.max_size_bytes or 0) // (1024 * 1024))
        )
        ctk.CTkEntry(size_frame, textvariable=self._max_size_var, width=100).grid(
            row=1, column=1, padx=8
        )

        ctk.CTkLabel(frame, text="Seuil 'gros fichier' par defaut (Mo)").pack(
            anchor="w", padx=8, pady=(16, 0)
        )
        self._big_threshold_var = ctk.StringVar(value=str(self._settings.big_file_threshold_mb))
        ctk.CTkEntry(frame, textvariable=self._big_threshold_var, width=120).pack(
            anchor="w", padx=8, pady=6
        )

        ctk.CTkLabel(frame, text="Seuil 'fichier ancien' par defaut (jours)").pack(
            anchor="w", padx=8, pady=(8, 0)
        )
        self._old_threshold_var = ctk.StringVar(value=str(self._settings.old_file_threshold_days))
        ctk.CTkEntry(frame, textvariable=self._old_threshold_var, width=120).pack(
            anchor="w", padx=8, pady=6
        )

    def _add_excluded_folder(self) -> None:
        folder = filedialog.askdirectory()
        if folder:
            self._excluded_box.insert("end", f"\n{folder}")

    # ------------------------------------------------------------------
    # Onglet Categories
    # ------------------------------------------------------------------

    def _build_categories_tab(self) -> None:
        frame = self._tab_categories
        ctk.CTkLabel(
            frame,
            text="Decochez une categorie pour la desactiver lors du classement automatique.",
        ).pack(anchor="w", padx=8, pady=(12, 8))

        scroll = ctk.CTkScrollableFrame(frame, height=380)
        scroll.pack(fill="both", expand=True, padx=8, pady=8)

        self._category_vars: dict[str, ctk.BooleanVar] = {}
        all_categories = {**DEFAULT_CATEGORIES, **self._settings.categories}
        for category in all_categories:
            enabled = category not in self._settings.disabled_categories
            var = ctk.BooleanVar(value=enabled)
            self._category_vars[category] = var
            ctk.CTkCheckBox(scroll, text=category, variable=var).pack(anchor="w", pady=4, padx=4)

    # ------------------------------------------------------------------

    def _save(self) -> None:
        self._settings.theme = self._theme_var.get()
        self._settings.language = self._lang_var.get()
        self._settings.ask_confirmation = self._confirm_var.get()
        self._settings.use_recycle_bin = self._recycle_var.get()
        self._settings.quarantine_folder = self._quarantine_var.get().strip() or None
        self._settings.max_workers = int(self._workers_var.get())

        excluded_text = self._excluded_box.get("1.0", "end").strip()
        self._settings.excluded_folders = [line.strip() for line in excluded_text.splitlines() if line.strip()]

        self._settings.ignored_extensions = [
            e.strip() if e.strip().startswith(".") else f".{e.strip()}"
            for e in self._ignored_ext_var.get().split(",")
            if e.strip()
        ]

        try:
            min_mb = int(self._min_size_var.get() or 0)
            self._settings.min_size_bytes = min_mb * 1024 * 1024 if min_mb > 0 else None
        except ValueError:
            self._settings.min_size_bytes = None

        try:
            max_mb = int(self._max_size_var.get() or 0)
            self._settings.max_size_bytes = max_mb * 1024 * 1024 if max_mb > 0 else None
        except ValueError:
            self._settings.max_size_bytes = None

        try:
            self._settings.big_file_threshold_mb = int(self._big_threshold_var.get())
        except ValueError:
            pass

        try:
            self._settings.old_file_threshold_days = int(self._old_threshold_var.get())
        except ValueError:
            pass

        self._settings.disabled_categories = [
            cat for cat, var in self._category_vars.items() if not var.get()
        ]

        save_settings(self._settings)
        self._on_saved(self._settings)
        self.destroy()
