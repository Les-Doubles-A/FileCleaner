//! Pages de l'application.

mod dupes;
mod explore;
mod home;
mod lists;
mod organize;
mod safety;
mod settings;
#[cfg(feature = "shots")]
pub mod shots;
mod system;

use std::sync::Arc;

use eframe::egui::Ui;
use filecleaner_core::config::Settings;
use filecleaner_core::model::Scan;

use crate::app::{Action, App};
use crate::icons;
use crate::jobs::PickPurpose;
use crate::theme::t;
use crate::widgets::empty_state;

/// Titre d'une section de la barre laterale et ses pages.
pub type PageGroup = (fn() -> &'static str, &'static [Page]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Home,
    Explore,
    Files,
    Duplicates,
    Similar,
    Junk,
    Big,
    Old,
    Empty,
    Rare,
    Dev,
    System,
    Organize,
    Rename,
    Quarantine,
    History,
    Settings,
}

impl Page {
    pub fn groups() -> [PageGroup; 4] {
        [
            (|| t("VUE D'ENSEMBLE", "OVERVIEW"), &[Page::Home, Page::Explore, Page::Files]),
            (
                || t("NETTOYER", "CLEAN UP"),
                &[Page::Duplicates, Page::Similar, Page::Junk, Page::Big, Page::Old, Page::Empty, Page::Rare, Page::Dev, Page::System],
            ),
            (|| t("ORGANISER", "ORGANIZE"), &[Page::Organize, Page::Rename]),
            (|| t("SÉCURITÉ", "SAFETY"), &[Page::Quarantine, Page::History]),
        ]
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Home => t("Tableau de bord", "Dashboard"),
            Self::Explore => t("Carte du disque", "Disk map"),
            Self::Files => t("Recherche", "Search"),
            Self::Duplicates => t("Doublons", "Duplicates"),
            Self::Similar => t("Images similaires", "Similar images"),
            Self::Junk => t("Fichiers inutiles", "Junk files"),
            Self::Big => t("Gros fichiers", "Large files"),
            Self::Old => t("Fichiers anciens", "Old files"),
            Self::Empty => t("Dossiers vides", "Empty folders"),
            Self::Rare => t("Extensions rares", "Rare extensions"),
            Self::Dev => t("Projets de dev", "Dev projects"),
            Self::System => t("Nettoyage Windows", "Windows cleanup"),
            Self::Organize => t("Classement", "Sorting"),
            Self::Rename => t("Renommage", "Renaming"),
            Self::Quarantine => t("Quarantaine", "Quarantine"),
            Self::History => t("Historique", "History"),
            Self::Settings => t("Réglages", "Settings"),
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Home => icons::SQUARES_FOUR,
            Self::Explore => icons::CHART_PIE,
            Self::Files => icons::MAGNIFYING_GLASS,
            Self::Duplicates => icons::COPY,
            Self::Similar => icons::IMAGES,
            Self::Junk => icons::BROOM,
            Self::Big => icons::STACK,
            Self::Old => icons::HOURGLASS,
            Self::Empty => icons::FOLDER_DASHED,
            Self::Rare => icons::PUZZLE_PIECE,
            Self::Dev => icons::CODE,
            Self::System => icons::WRENCH,
            Self::Organize => icons::TREE_STRUCTURE,
            Self::Rename => icons::PENCIL_SIMPLE,
            Self::Quarantine => icons::VAULT,
            Self::History => icons::CLOCK_COUNTER_CLOCKWISE,
            Self::Settings => icons::GEAR_SIX,
        }
    }

    pub fn shows_inspector(self) -> bool {
        matches!(
            self,
            Self::Explore | Self::Files | Self::Duplicates | Self::Similar | Self::Junk | Self::Big | Self::Old | Self::Rare
        )
    }
}

/// Etat local des pages.
pub struct PagesState {
    pub focus_search: bool,
    pub search: String,
    pub big_mb: u64,
    pub big_rows: Vec<u32>,
    pub old_days: u32,
    pub old_rows: Vec<u32>,
    pub rare_max: u32,
    pub rare_rows: Vec<u32>,
    pub rows_generation: u64,
    pub search_rows: Vec<u32>,
    pub search_key: Option<(String, u64)>,
    pub search_problems: Vec<String>,
    pub junk_filter: Option<usize>,
    pub junk_rows: Vec<u32>,
    pub junk_key: Option<(Option<usize>, u64)>,
    pub empty_selected: std::collections::HashSet<u32>,
    pub empty_generation: u64,
    pub dupes: dupes::DupesState,
    pub explore: explore::ExploreState,
    pub system: system::SystemState,
    pub organize: organize::OrganizeState,
    pub rename: organize::RenameState,
    pub settings: settings::SettingsState,
}

impl PagesState {
    pub fn new(settings: &Settings) -> Self {
        Self {
            focus_search: false,
            search: String::new(),
            big_mb: settings.big_file_threshold_mb,
            big_rows: Vec::new(),
            old_days: settings.old_file_threshold_days,
            old_rows: Vec::new(),
            rare_max: settings.rare_ext_max,
            rare_rows: Vec::new(),
            rows_generation: 0,
            search_rows: Vec::new(),
            search_key: None,
            search_problems: Vec::new(),
            junk_filter: None,
            junk_rows: Vec::new(),
            junk_key: None,
            empty_selected: Default::default(),
            empty_generation: 0,
            dupes: Default::default(),
            explore: Default::default(),
            system: Default::default(),
            organize: organize::OrganizeState::new(settings),
            rename: organize::RenameState::new(settings),
            settings: Default::default(),
        }
    }

    pub fn on_new_scan(&mut self, scan: &Scan) {
        self.rows_generation = 0;
        self.search_key = None;
        self.junk_key = None;
        self.junk_filter = None;
        self.empty_generation = 0;
        self.explore = Default::default();
        self.organize.on_new_scan(scan);
        self.rename.invalidate();
    }
}

/// Retourne le scan courant ou affiche l'invitation a analyser.
pub fn require_scan(ui: &mut Ui, app: &mut App) -> Option<Arc<Scan>> {
    if let Some(scan) = &app.scan {
        return Some(scan.clone());
    }
    let title = app.page.title();
    let busy = app.is_busy();
    let action = (!busy).then(|| t("Choisir un dossier", "Choose a folder"));
    let message = if busy {
        t("Analyse en cours, un instant...", "Scanning, one moment...")
    } else {
        t("Analysez un dossier pour commencer.", "Scan a folder to get started.")
    };
    if empty_state(ui, app.page.icon(), title, message, action) {
        app.actions.push(Action::Pick(PickPurpose::Scan));
    }
    None
}

pub fn show(ui: &mut Ui, app: &mut App) {
    match app.page {
        Page::Home => home::show(ui, app),
        Page::Explore => explore::show(ui, app),
        Page::Files => lists::files(ui, app),
        Page::Duplicates => dupes::duplicates(ui, app),
        Page::Similar => dupes::similar(ui, app),
        Page::Junk => lists::junk(ui, app),
        Page::Big => lists::big(ui, app),
        Page::Old => lists::old(ui, app),
        Page::Empty => lists::empty(ui, app),
        Page::Rare => lists::rare(ui, app),
        Page::Dev => lists::dev(ui, app),
        Page::System => system::show(ui, app),
        Page::Organize => organize::organize(ui, app),
        Page::Rename => organize::rename(ui, app),
        Page::Quarantine => safety::quarantine(ui, app),
        Page::History => safety::history(ui, app),
        Page::Settings => settings::show(ui, app),
    }
}
