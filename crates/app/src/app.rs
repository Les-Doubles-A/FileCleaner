//! Etat global, boucle de rendu et coquille de l'application.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Key, Layout, Margin, Modifiers, Panel, Pos2, Rect,
    RichText, Sense, Stroke, ThemePreference, Ui, Vec2,
};
use filecleaner_core::actions::{self, ActionReport, DeleteMode, DeleteRequest, Target};
use filecleaner_core::config::{AppPaths, Settings, Theme};
use filecleaner_core::duplicates::{self, DupGroup, DupMode, HashCache};
use filecleaner_core::export;
use filecleaner_core::history::History;
use filecleaner_core::model::{FileId, Scan};
use filecleaner_core::scanner;
use filecleaner_core::similar::{self, SimilarGroup};
use filecleaner_core::snapshot::Snapshots;
use filecleaner_core::system;
use filecleaner_core::util::{format_count, format_size};

use crate::dialogs::{Palette as CommandPalette, Review, Toast, ToastKind};
use crate::icons;
use crate::jobs::{Derived, ExportFormat, JobResult, Jobs, PickPurpose};
use crate::pages::{Page, PagesState};
use crate::preview::Previews;
use crate::table::{Env, Selection, TableState};
use crate::theme::{self, body, lang, pal, semibold, t};
use crate::widgets::{Kind, button, icon_button, paint_text, progress_bar};

pub enum Action {
    Navigate(Page),
    OpenPath(PathBuf),
    Reveal(PathBuf),
    ExcludeFolder(PathBuf),
    Scan(Vec<PathBuf>),
    Rescan,
    Pick(PickPurpose),
    ReviewSelection,
    Undo(String),
    UndoLast,
    Export(ExportFormat),
    FindDuplicates(DupMode),
    FindSimilar,
    ExpressClean,
}

pub struct App {
    pub paths: AppPaths,
    pub settings: Settings,
    pub settings_dirty: bool,
    pub history: History,
    pub snapshots: Snapshots,
    pub jobs: Jobs,
    pub scan: Option<Arc<Scan>>,
    pub derived: Option<Arc<Derived>>,
    pub dup_groups: Vec<DupGroup>,
    pub dup_mode: DupMode,
    pub dup_done: bool,
    pub similar: Vec<SimilarGroup>,
    pub similar_done: bool,
    pub sel: Selection,
    pub focus: Option<FileId>,
    pub page: Page,
    pub tables: HashMap<Page, TableState>,
    pub actions: Vec<Action>,
    pub toasts: Vec<Toast>,
    pub review: Option<Review>,
    pub command_palette: Option<CommandPalette>,
    pub previews: Previews,
    pub pages: PagesState,
    pub drives: Vec<system::Drive>,
    pending_roots: Vec<PathBuf>,
    applied_scale: f32,
    applied_accent: [u8; 3],
    #[cfg(feature = "shots")]
    pub shots: crate::pages::shots::Shots,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: AppPaths, roots: Vec<PathBuf>) -> Self {
        let ctx = cc.egui_ctx.clone();
        let settings = Settings::load(&paths);
        theme::set_lang(settings.language);
        theme::install_fonts(&ctx);
        theme::apply(&ctx, settings.accent);
        ctx.set_theme(theme_preference(settings.theme));

        let mut history = History::load(&paths.history_file());
        if settings.quarantine_retention_days > 0 {
            let freed = history.purge_quarantine(Some(settings.quarantine_retention_days));
            if freed > 0 {
                log::info!("quarantaine : {freed} octets purges automatiquement");
            }
        }
        let snapshots = Snapshots::load(&paths.snapshots_file());
        let cache = HashCache::load(&paths.hash_cache_file());
        let jobs = Jobs::new(ctx, cache);
        jobs.quiet(|| JobResult::Drives(system::drives()));

        Self {
            applied_scale: 0.0,
            applied_accent: settings.accent,
            pages: PagesState::new(&settings),
            paths,
            settings,
            settings_dirty: false,
            history,
            snapshots,
            jobs,
            scan: None,
            derived: None,
            dup_groups: Vec::new(),
            dup_mode: DupMode::Content,
            dup_done: false,
            similar: Vec::new(),
            similar_done: false,
            sel: Selection::default(),
            focus: None,
            page: Page::Home,
            tables: HashMap::new(),
            actions: Vec::new(),
            toasts: Vec::new(),
            review: None,
            command_palette: None,
            previews: Previews::default(),
            drives: Vec::new(),
            pending_roots: roots,
            #[cfg(feature = "shots")]
            shots: crate::pages::shots::Shots::from_env(),
        }
    }

    pub fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toasts.push(Toast::new(kind, text.into(), None));
    }

    pub fn is_busy(&self) -> bool {
        self.jobs.running.is_some()
    }

    pub fn start_scan(&mut self, roots: Vec<PathBuf>) {
        let roots: Vec<PathBuf> = roots.into_iter().filter(|r| scanner::is_scannable(r)).collect();
        if roots.is_empty() {
            self.toast(ToastKind::Error, t("Ce dossier n'est pas accessible.", "This folder is not accessible."));
            return;
        }
        for root in &roots {
            self.settings.push_recent(&root.to_string_lossy());
        }
        self.settings_dirty = true;
        let settings = self.settings.clone();
        let started = self.jobs.start(t("Analyse en cours", "Scanning"), true, move |progress| {
            JobResult::Scan(Box::new(scanner::scan(&roots, &settings, progress)))
        });
        if started {
            self.page = Page::Home;
            self.focus = None;
        } else {
            self.toast(ToastKind::Info, t("Une tâche est déjà en cours.", "A task is already running."));
        }
    }

    pub fn refresh_derived(&self) {
        if let Some(scan) = self.scan.clone() {
            let settings = self.settings.clone();
            self.jobs.quiet(move || JobResult::Derived(Box::new(Derived::compute(&scan, &settings))));
        }
    }

    pub fn find_duplicates(&mut self, mode: DupMode) {
        let Some(scan) = self.scan.clone() else { return };
        let generation = scan.generation;
        self.dup_mode = mode;
        match mode {
            DupMode::ByName => {
                self.dup_groups = duplicates::find_by_name(&scan);
                self.dup_done = true;
                self.pages.dupes.rebuild(&self.dup_groups);
            }
            DupMode::Content => {
                let cache = self.jobs.hash_cache.clone();
                let min = self.settings.min_duplicate_bytes;
                let cache_file = self.paths.hash_cache_file();
                self.jobs.start(t("Recherche des doublons", "Finding duplicates"), true, move |progress| {
                    let groups = duplicates::find_by_content(&scan, min, &cache, progress);
                    if let Ok(mut guard) = cache.lock() {
                        guard.save(&cache_file);
                    }
                    JobResult::Duplicates(groups, DupMode::Content, generation)
                });
            }
        }
    }

    fn find_similar(&mut self) {
        let Some(scan) = self.scan.clone() else { return };
        let threshold = self.settings.similar_threshold;
        self.jobs.start(t("Comparaison des images", "Comparing images"), true, move |progress| {
            JobResult::Similar(similar::find_similar(&scan, threshold, progress))
        });
    }

    /// Cibles de la selection courante, dossiers d'abord.
    fn selection_targets(&self) -> Vec<Target> {
        let Some(scan) = &self.scan else { return Vec::new() };
        let dirs: Vec<u32> = self.sel.dirs.iter().copied().collect();
        let inside_selected = |path: &Path| dirs.iter().any(|&d| path.starts_with(&scan.dir(d).path));
        let mut targets: Vec<Target> = dirs.iter().map(|&d| Target::from_dir(scan, d)).collect();
        targets.extend(
            self.sel
                .files
                .iter()
                .filter(|&&f| !inside_selected(scan.folder_of(f)))
                .map(|&f| Target::from_file(scan, f)),
        );
        targets
    }

    pub fn open_review_for_selection(&mut self) {
        if self.sel.is_empty() {
            return;
        }
        let targets = self.selection_targets();
        let mut warnings = Vec::new();
        let full = duplicates::fully_selected(&self.dup_groups, &self.sel.files);
        if full > 0 {
            warnings.push(format!(
                "{} {}",
                full,
                t(
                    "groupe(s) de doublons ont tous leurs exemplaires sélectionnés : aucune copie ne restera.",
                    "duplicate group(s) have every copy selected: no copy will remain."
                )
            ));
        }
        self.review = Some(Review::new(
            t("Nettoyage de la sélection", "Selection cleanup").to_string(),
            targets,
            self.settings.delete_mode,
            warnings,
        ));
    }

    pub fn run_delete(&mut self, label: String, targets: Vec<Target>, mode: DeleteMode) {
        let roots = self.scan.as_ref().map(|s| s.roots.clone()).unwrap_or_default();
        let root = self.scan.as_ref().map(|s| s.title()).unwrap_or_default();
        let settings = self.settings.clone();
        let language = lang();
        self.jobs.start(t("Nettoyage en cours", "Cleaning up"), true, move |progress| {
            let request = DeleteRequest { label: &label, root: &root, roots: &roots, mode, lang: language };
            JobResult::Delete(actions::delete_targets(&targets, &request, &settings, progress))
        });
    }

    fn commit_session(&mut self, report: &mut ActionReport) -> Option<String> {
        let session = report.session.take()?;
        let undoable = session.restorable_count() > 0;
        let id = session.id.clone();
        if let Err(err) = self.history.commit(session) {
            log::warn!("historique non enregistre : {err:#}");
        }
        undoable.then_some(id)
    }

    fn report_failures(&mut self, failed: &[(PathBuf, String)]) {
        for (path, message) in failed.iter().take(50) {
            log::warn!("echec sur {} : {message}", path.display());
        }
        if let Some((path, message)) = failed.first() {
            let extra = failed.len() - 1;
            let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
            let suffix = if extra > 0 { format!(" (+{extra})") } else { String::new() };
            self.toast(ToastKind::Error, format!("{name} : {message}{suffix}"));
        }
    }

    fn handle_result(&mut self, ctx: &egui::Context, result: JobResult) {
        match result {
            JobResult::Scan(scan) => self.on_scan(*scan),
            JobResult::Derived(derived) => {
                let fresh = self.scan.as_ref().is_some_and(|s| s.generation == derived.generation);
                if fresh {
                    self.derived = Some(Arc::new(*derived));
                }
            }
            JobResult::Duplicates(groups, mode, generation) => {
                self.jobs.running = None;
                if self.scan.as_ref().is_some_and(|s| s.generation >= generation) {
                    self.dup_groups = groups;
                    self.dup_mode = mode;
                    self.dup_done = true;
                    self.pages.dupes.rebuild(&self.dup_groups);
                }
            }
            JobResult::Similar(groups) => {
                self.jobs.running = None;
                self.similar = groups;
                self.similar_done = true;
            }
            JobResult::Delete(mut report) => {
                self.jobs.running = None;
                let session = self.commit_session(&mut report);
                self.apply_removals(&report);
                self.report_failures(&report.failed);
                if report.succeeded > 0 {
                    let text = format!(
                        "{} {} · {}",
                        format_count(report.succeeded as u64, lang()),
                        t("élément(s) nettoyé(s)", "item(s) cleaned"),
                        format_size(report.freed_bytes, lang()),
                    );
                    self.toasts.push(Toast::new(ToastKind::Success, text, session));
                }
                self.pages.system.needs_measure = true;
                self.request_recycle_stats();
            }
            JobResult::EmptyDirs(mut report) => {
                self.jobs.running = None;
                let session = self.commit_session(&mut report);
                self.apply_removals(&report);
                self.report_failures(&report.failed);
                if report.succeeded > 0 {
                    let text = format!("{} {}", report.succeeded, t("dossier(s) retiré(s)", "folder(s) removed"));
                    self.toasts.push(Toast::new(ToastKind::Success, text, session));
                }
            }
            JobResult::Moved(mut report) => {
                self.jobs.running = None;
                let session = self.commit_session(&mut report);
                self.report_failures(&report.failed);
                if report.succeeded > 0 {
                    let text = format!("{} {}", format_count(report.succeeded as u64, lang()), t("fichier(s) déplacé(s)", "file(s) moved"));
                    self.toasts.push(Toast::new(ToastKind::Success, text, session));
                }
                self.sel.clear();
                self.actions.push(Action::Rescan);
            }
            JobResult::Renamed(mut report) => {
                self.jobs.running = None;
                let session = report.session.take();
                let id = session.as_ref().map(|s| s.id.clone());
                if let Some(session) = session
                    && let Err(err) = self.history.commit(session) {
                        log::warn!("historique non enregistre : {err:#}");
                    }
                if let Some(scan) = &mut self.scan {
                    Arc::make_mut(scan).apply_renames(&report.renamed);
                }
                self.report_failures(&report.failed);
                if !report.renamed.is_empty() {
                    let text = format!("{} {}", format_count(report.renamed.len() as u64, lang()), t("fichier(s) renommé(s)", "file(s) renamed"));
                    self.toasts.push(Toast::new(ToastKind::Success, text, id));
                }
                self.pages.rename.invalidate();
                self.refresh_derived();
            }
            JobResult::Undone(report) => {
                self.jobs.running = None;
                self.history = History::load(&self.paths.history_file());
                let kind = if report.failed > 0 { ToastKind::Error } else { ToastKind::Success };
                let text = format!(
                    "{} {}, {} {}",
                    report.restored,
                    t("élément(s) restauré(s)", "item(s) restored"),
                    report.failed,
                    t("échec(s)", "failed"),
                );
                self.toast(kind, text);
                if report.restored > 0 {
                    self.actions.push(Action::Rescan);
                }
            }
            JobResult::Measured(id, measure) => {
                self.pages.system.measured.insert(id, measure);
            }
            JobResult::RecycleBin(stats) => self.pages.system.recycle = stats,
            JobResult::RecycleEmptied(result) => {
                self.jobs.running = None;
                match result {
                    Ok(()) => self.toast(ToastKind::Success, t("Corbeille vidée.", "Recycle Bin emptied.")),
                    Err(err) => self.toast(ToastKind::Error, err),
                }
                self.request_recycle_stats();
            }
            JobResult::Exported(result) => {
                self.jobs.running = None;
                match result {
                    Ok(path) => {
                        let text = format!("{} {}", t("Rapport créé :", "Report created:"), path.display());
                        self.toast(ToastKind::Success, text);
                        self.actions.push(Action::Reveal(path));
                    }
                    Err(err) => self.toast(ToastKind::Error, err),
                }
            }
            JobResult::PickedFolders(purpose, folders) => self.on_picked(purpose, folders),
            JobResult::PickedSave(format, path) => {
                if let Some(path) = path {
                    self.run_export(format, path);
                }
            }
            JobResult::Drives(drives) => self.drives = drives,
            JobResult::Thumbnail(path, image) => self.previews.store_thumbnail(ctx, path, image),
            JobResult::TextPreview(path, text) => self.previews.store_text(path, text),
        }
    }

    pub fn request_recycle_stats(&self) {
        self.jobs.quiet(|| JobResult::RecycleBin(system::recycle_bin_stats()));
    }

    fn on_scan(&mut self, scan: Scan) {
        self.jobs.running = None;
        if scan.cancelled {
            self.toast(ToastKind::Info, t("Analyse annulée.", "Scan cancelled."));
            return;
        }
        if !scan.errors.is_empty() {
            for err in scan.errors.iter().take(20) {
                log::debug!("scan: {err}");
            }
            let text = format!("{} {}", scan.errors.len(), t("dossier(s) illisible(s) ignoré(s).", "unreadable folder(s) skipped."));
            self.toast(ToastKind::Info, text);
        }
        self.snapshots.record(&scan);
        if let Err(err) = self.snapshots.save(&self.paths.snapshots_file()) {
            log::warn!("instantanes non enregistres : {err:#}");
        }
        self.sel.clear();
        self.focus = None;
        self.dup_groups.clear();
        self.dup_done = false;
        self.pages.dupes.rebuild(&[]);
        self.similar.clear();
        self.similar_done = false;
        self.tables.clear();
        self.pages.on_new_scan(&scan);
        self.derived = None;
        self.scan = Some(Arc::new(scan));
        self.refresh_derived();
        if self.settings.auto_duplicates {
            self.find_duplicates(DupMode::Content);
        }
    }

    fn apply_removals(&mut self, report: &ActionReport) {
        let Some(scan_arc) = &mut self.scan else { return };
        if report.done_files.is_empty() && report.done_dirs.is_empty() {
            return;
        }
        let scan = Arc::make_mut(scan_arc);
        let files: HashSet<FileId> = report.done_files.iter().copied().collect();
        scan.mark_files_deleted(&files);
        for &dir in &report.done_dirs {
            scan.mark_dir_deleted(dir);
        }
        self.sel.prune(scan);
        if self.focus.is_some_and(|f| !scan.file(f).is_alive()) {
            self.focus = None;
        }
        for group in &mut self.dup_groups {
            group.files.retain(|&f| scan.file(f).is_alive());
            group.physical_copies = group.physical_copies.min(group.files.len());
        }
        self.dup_groups.retain(|g| g.files.len() > 1);
        self.pages.dupes.rebuild(&self.dup_groups);
        for group in &mut self.similar {
            group.items.retain(|i| scan.file(i.file).is_alive());
        }
        self.similar.retain(|g| g.items.len() > 1);
        self.refresh_derived();
    }

    fn on_picked(&mut self, purpose: PickPurpose, folders: Vec<PathBuf>) {
        let Some(first) = folders.first().cloned() else { return };
        match purpose {
            PickPurpose::Scan => self.actions.push(Action::Scan(folders)),
            PickPurpose::AddRoot => {
                let mut roots = self.scan.as_ref().map(|s| s.roots.clone()).unwrap_or_default();
                roots.extend(folders);
                self.actions.push(Action::Scan(roots));
            }
            PickPurpose::Quarantine => {
                self.settings.quarantine_folder = Some(first.display().to_string());
                self.settings_dirty = true;
            }
            PickPurpose::Exclude => {
                self.settings.excluded_folders.push(first.display().to_string());
                self.settings_dirty = true;
            }
            PickPurpose::MoveSelection => self.move_selection(&first),
            PickPurpose::KeepFolder => self.pages.dupes.keep_folder = Some(first),
            PickPurpose::OrganizeDestination => self.pages.organize.set_destination(first),
        }
    }

    fn move_selection(&mut self, folder: &Path) {
        let Some(scan) = self.scan.clone() else { return };
        let moves: Vec<(PathBuf, PathBuf)> = self
            .sel
            .files
            .iter()
            .map(|&id| (scan.file_path(id), folder.join(&*scan.file(id).name)))
            .collect();
        let root = scan.title();
        let label = t("Déplacement de fichiers", "Moved files").to_string();
        self.jobs.start(t("Déplacement", "Moving"), true, move |progress| {
            JobResult::Moved(actions::move_files(&moves, &label, &root, progress))
        });
    }

    fn pick_folder(&self, purpose: PickPurpose) {
        let many = matches!(purpose, PickPurpose::Scan | PickPurpose::AddRoot);
        self.jobs.quiet(move || {
            let dialog = rfd::FileDialog::new();
            let folders = if many {
                dialog.pick_folders().unwrap_or_default()
            } else {
                dialog.pick_folder().into_iter().collect()
            };
            JobResult::PickedFolders(purpose, folders)
        });
    }

    fn run_export(&mut self, format: ExportFormat, path: PathBuf) {
        let Some(scan) = self.scan.clone() else { return };
        let settings = self.settings.clone();
        self.jobs.start(t("Export du rapport", "Exporting report"), false, move |_| {
            let result = match format {
                ExportFormat::Csv => export::export_csv(&scan, &path),
                ExportFormat::Json => export::export_json(&scan, &path),
                ExportFormat::Html => export::export_html(&scan, &settings, &path),
                ExportFormat::Pdf => export::export_pdf(&scan, &settings, &path),
            };
            JobResult::Exported(result.map(|()| path.clone()).map_err(|err| format!("{err:#}")))
        });
    }

    fn undo(&mut self, session: String) {
        self.toasts.retain(|toast| !toast.offers_undo_of(&session));
        let file = self.paths.history_file();
        self.jobs.start(t("Restauration", "Restoring"), false, move |_| {
            let mut history = History::load(&file);
            JobResult::Undone(history.undo(&session))
        });
    }

    fn process_actions(&mut self) {
        for action in std::mem::take(&mut self.actions) {
            match action {
                Action::Navigate(page) => self.navigate(page),
                Action::OpenPath(path) => open_path(&path),
                Action::Reveal(path) => reveal(&path),
                Action::ExcludeFolder(path) => {
                    self.settings.excluded_folders.push(path.display().to_string());
                    self.settings_dirty = true;
                    self.toast(ToastKind::Info, t("Dossier exclu des prochaines analyses.", "Folder excluded from future scans."));
                }
                Action::Scan(roots) => self.start_scan(roots),
                Action::Rescan => {
                    if let Some(scan) = &self.scan {
                        let roots = scan.roots.clone();
                        let page = self.page;
                        self.start_scan(roots);
                        self.page = page;
                    }
                }
                Action::Pick(purpose) => self.pick_folder(purpose),
                Action::ReviewSelection => self.open_review_for_selection(),
                Action::Undo(session) => self.undo(session),
                Action::UndoLast => match self.history.last_restorable().map(|s| s.id.clone()) {
                    Some(id) => self.undo(id),
                    None => self.toast(ToastKind::Info, t("Rien à annuler.", "Nothing to undo.")),
                },
                Action::Export(format) => self.ask_export(format),
                Action::FindDuplicates(mode) => self.find_duplicates(mode),
                Action::FindSimilar => self.find_similar(),
                Action::ExpressClean => self.express_clean(),
            }
        }
    }

    /// Selectionne d'un coup tout ce qui est sans risque, puis ouvre la revue.
    fn express_clean(&mut self) {
        use filecleaner_core::analysis::{Risk, rule_risk};
        let (Some(scan), Some(derived)) = (self.scan.clone(), self.derived.clone()) else { return };
        for hit in derived.junk.iter().filter(|h| rule_risk(h.rule) == Risk::Safe) {
            self.sel.set(&scan, hit.file, true);
        }
        if self.sel.is_empty() {
            self.toast(ToastKind::Info, t("Rien à nettoyer sans risque ici.", "Nothing safe to clean here."));
        } else {
            self.open_review_for_selection();
        }
    }

    fn ask_export(&mut self, format: ExportFormat) {
        let Some(scan) = &self.scan else { return };
        let stem = scan
            .roots
            .first()
            .and_then(|r| r.file_name())
            .map_or_else(|| "scan".to_string(), |n| n.to_string_lossy().into_owned());
        let name = format!("filecleaner_{stem}.{}", format.extension());
        self.jobs.quiet(move || {
            let path = rfd::FileDialog::new()
                .set_file_name(name)
                .add_filter(format.extension().to_uppercase(), &[format.extension()])
                .save_file();
            JobResult::PickedSave(format, path)
        });
    }

    pub fn navigate(&mut self, page: Page) {
        self.page = page;
        self.focus = None;
        if page == Page::System {
            self.pages.system.needs_measure = true;
            self.request_recycle_stats();
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let modal_open = self.review.is_some() || self.command_palette.is_some();
        let (palette, open, refresh, undo, find, delete, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND, Key::K),
                i.consume_key(Modifiers::COMMAND, Key::O),
                i.consume_key(Modifiers::NONE, Key::F5),
                i.consume_key(Modifiers::COMMAND, Key::Z),
                i.consume_key(Modifiers::COMMAND, Key::F),
                i.key_pressed(Key::Delete),
                i.key_pressed(Key::Escape),
            )
        });
        if palette {
            self.command_palette = match self.command_palette {
                Some(_) => None,
                None => Some(CommandPalette::default()),
            };
        }
        if modal_open {
            return;
        }
        let typing = ctx.egui_wants_keyboard_input();
        if open {
            self.actions.push(Action::Pick(PickPurpose::Scan));
        }
        if refresh {
            self.actions.push(Action::Rescan);
        }
        if undo && !typing {
            self.actions.push(Action::UndoLast);
        }
        if find {
            self.navigate(Page::Files);
            self.pages.focus_search = true;
        }
        if delete && !typing {
            self.actions.push(Action::ReviewSelection);
        }
        if escape && !typing {
            self.focus = None;
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if dropped.is_empty() {
            return;
        }
        let folders: Vec<PathBuf> = dropped
            .into_iter()
            .map(|p| if p.is_dir() { p } else { p.parent().map(Path::to_path_buf).unwrap_or(p) })
            .collect();
        self.actions.push(Action::Scan(folders));
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        let p = pal(ui);
        Panel::left("sidebar")
            .exact_size(232.0)
            .show_separator_line(false)
            .frame(Frame::new().fill(p.sidebar).inner_margin(Margin::symmetric(10, 12)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
                    ui.painter().rect_filled(rect, CornerRadius::same(8), p.accent);
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, icons::BROOM, body(17.0), p.on_accent);
                    ui.label(RichText::new("File Cleaner").font(semibold(16.0)).color(p.text));
                });
                ui.add_space(6.0);
                Panel::bottom("sidebar-foot").show_separator_line(false).frame(Frame::NONE).show(ui, |ui| {
                    ui.add_space(6.0);
                    if nav_item(ui, Page::Settings.icon(), Page::Settings.title(), self.page == Page::Settings, None) {
                        self.actions.push(Action::Navigate(Page::Settings));
                    }
                });
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    for (group, pages) in Page::groups() {
                        ui.add_space(7.0);
                        ui.horizontal(|ui| {
                            ui.add_space(10.0);
                            ui.label(RichText::new(group()).size(11.5).color(p.faint));
                        });
                        ui.add_space(2.0);
                        for &page in pages {
                            let badge = self.nav_badge(page);
                            if nav_item(ui, page.icon(), page.title(), self.page == page, badge.as_deref()) {
                                self.actions.push(Action::Navigate(page));
                            }
                        }
                    }
                });
            });
    }

    fn nav_badge(&self, page: Page) -> Option<String> {
        let size = |bytes: u64| (bytes > 0).then(|| format_size(bytes, lang()));
        let count = |n: usize| (n > 0).then(|| format_count(n as u64, lang()));
        let derived = self.derived.as_ref();
        match page {
            Page::Duplicates => size(self.dup_groups.iter().map(DupGroup::recoverable).sum()),
            Page::Junk => derived.and_then(|d| size(d.insights.junk_bytes)),
            Page::Dev => derived.and_then(|d| size(d.insights.dev_bytes)),
            Page::Big => derived.and_then(|d| size(d.insights.big_bytes)),
            Page::Empty => derived.and_then(|d| count(d.empty.len())),
            Page::Similar => count(self.similar.len()),
            Page::Quarantine => size(self.history.quarantine_bytes()),
            _ => None,
        }
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        let p = pal(ui);
        Panel::top("topbar")
            .exact_size(52.0)
            .show_separator_line(false)
            .frame(Frame::new().fill(p.bg).inner_margin(Margin::symmetric(20, 10)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    if let Some(scan) = self.scan.clone() {
                        let title = scan.title();
                        ui.label(RichText::new(icons::FOLDER_OPEN).size(16.0).color(p.muted));
                        let width = (ui.available_width() - 430.0).max(120.0);
                        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 24.0), Sense::hover());
                        paint_text(ui, ui.painter(), rect.left_center(), &title, semibold(14.0), p.text, rect.width());
                        response.on_hover_text(&title);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.scan.is_some() {
                            ui.menu_button(format!("{}  {}", icons::EXPORT, t("Exporter", "Export")), |ui| {
                                for (format, label) in [
                                    (ExportFormat::Html, t("Rapport HTML", "HTML report")),
                                    (ExportFormat::Pdf, t("Rapport PDF", "PDF report")),
                                    (ExportFormat::Csv, t("Liste CSV", "CSV list")),
                                    (ExportFormat::Json, t("Données JSON", "JSON data")),
                                ] {
                                    if ui.button(label).clicked() {
                                        self.actions.push(Action::Export(format));
                                    }
                                }
                            });
                            if icon_button(ui, icons::ARROWS_CLOCKWISE, t("Relancer l'analyse (F5)", "Scan again (F5)")).clicked() {
                                self.actions.push(Action::Rescan);
                            }
                            if icon_button(ui, icons::FOLDER_PLUS, t("Ajouter un dossier à l'analyse", "Add a folder to the scan")).clicked() {
                                self.actions.push(Action::Pick(PickPurpose::AddRoot));
                            }
                        } else if button(ui, Kind::Primary, &format!("{}  {}", icons::FOLDER_OPEN, t("Choisir un dossier", "Choose a folder"))).clicked() {
                            self.actions.push(Action::Pick(PickPurpose::Scan));
                        }
                        let hint = format!("{}  {}  Ctrl K", icons::MAGNIFYING_GLASS, t("Commandes", "Commands"));
                        if button(ui, Kind::Secondary, &hint).clicked() {
                            self.command_palette = Some(CommandPalette::default());
                        }
                    });
                });
            });
    }

    fn selection_bar(&mut self, ui: &mut Ui) {
        if self.sel.is_empty() {
            return;
        }
        let p = pal(ui);
        Panel::bottom("selection")
            .exact_size(60.0)
            .show_separator_line(false)
            .frame(Frame::new().fill(p.card).stroke(Stroke::new(1.0, p.line)).inner_margin(Margin::symmetric(20, 12)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new(icons::CHECK_SQUARE).size(18.0).color(p.accent));
                    ui.label(
                        RichText::new(format!(
                            "{} {}",
                            format_count(self.sel.count() as u64, lang()),
                            t("sélectionné(s)", "selected")
                        ))
                        .font(semibold(14.0))
                        .color(p.text),
                    );
                    ui.label(RichText::new(format_size(self.sel.bytes, lang())).color(p.muted));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let label = format!("{}  {}", icons::BROOM, t("Nettoyer", "Clean up"));
                        if button(ui, Kind::Primary, &label).clicked() {
                            self.actions.push(Action::ReviewSelection);
                        }
                        if !self.sel.files.is_empty() && self.sel.dirs.is_empty() {
                            let label = format!("{}  {}", icons::ARROW_RIGHT, t("Déplacer vers", "Move to"));
                            if button(ui, Kind::Secondary, &label).clicked() {
                                self.actions.push(Action::Pick(PickPurpose::MoveSelection));
                            }
                            if button(ui, Kind::Secondary, &format!("{}  {}", icons::PENCIL_SIMPLE, t("Renommer", "Rename"))).clicked() {
                                self.pages.rename.use_selection();
                                self.actions.push(Action::Navigate(Page::Rename));
                            }
                        }
                        if button(ui, Kind::Ghost, t("Tout désélectionner", "Clear selection")).clicked() {
                            self.sel.clear();
                        }
                    });
                });
            });
    }

    fn progress_strip(&mut self, ui: &mut Ui) {
        let Some(running) = &self.jobs.running else { return };
        // L'analyse dispose de son propre ecran d'avancement.
        if self.page == Page::Home && self.scan.is_none() {
            return;
        }
        let p = pal(ui);
        let (done, total, _, _, current) = running.progress.snapshot();
        let title = running.title.clone();
        let cancellable = running.cancellable;
        let progress = running.progress.clone();
        let value = (total > 0).then(|| done as f32 / total as f32);
        Panel::top("progress")
            .exact_size(44.0)
            .show_separator_line(false)
            .frame(Frame::new().fill(p.bg).inner_margin(Margin::symmetric(20, 4)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(title).color(p.text));
                    let width = (ui.available_width() - 90.0).max(40.0);
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 18.0), Sense::hover());
                    paint_text(ui, ui.painter(), rect.left_center(), &current, body(12.0), p.faint, width);
                    if cancellable && button(ui, Kind::Ghost, t("Annuler", "Cancel")).clicked() {
                        progress.cancel();
                    }
                });
                progress_bar(ui, value, 3.0);
            });
        ui.ctx().request_repaint_after(Duration::from_millis(100));
    }

    fn show_inspector(&mut self, ui: &mut Ui) {
        if self.focus.is_none() || !self.page.shows_inspector() {
            return;
        }
        let Some(scan) = self.scan.clone() else { return };
        let p = pal(ui);
        Panel::right("inspector")
            .default_size(320.0)
            .size_range(260.0..=520.0)
            .show_separator_line(false)
            .frame(Frame::new().fill(p.card).stroke(Stroke::new(1.0, p.line)).inner_margin(Margin::same(16)))
            .show(ui, |ui| {
                crate::preview::inspector(ui, self, &scan);
            });
    }

    pub fn env<'a>(
        scan: &'a Scan,
        sel: &'a mut Selection,
        focus: &'a mut Option<FileId>,
        actions: &'a mut Vec<Action>,
        compact: bool,
    ) -> Env<'a> {
        Env { scan, sel, focus, actions, row_height: if compact { 30.0 } else { 38.0 } }
    }

    fn sync_appearance(&mut self, ctx: &egui::Context) {
        if (self.applied_scale - self.settings.ui_scale).abs() > f32::EPSILON {
            self.applied_scale = self.settings.ui_scale;
            ctx.set_zoom_factor(self.settings.ui_scale);
        }
        if self.applied_accent != self.settings.accent {
            self.applied_accent = self.settings.accent;
            theme::apply(ctx, self.settings.accent);
        }
        theme::set_lang(self.settings.language);
    }
}

fn nav_item(ui: &mut Ui, icon: &str, label: &str, selected: bool, badge: Option<&str>) -> bool {
    let p = pal(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let (fill, fg) = if selected {
        (p.accent_soft, p.text)
    } else if response.hovered() {
        (p.hover, p.text)
    } else {
        (Color32::TRANSPARENT, p.muted)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(theme::RADIUS), fill);
    if selected {
        let bar = Rect::from_min_size(Pos2::new(rect.left(), rect.center().y - 8.0), Vec2::new(3.0, 16.0));
        painter.rect_filled(bar, CornerRadius::same(2), p.accent);
    }
    let y = rect.center().y;
    let icon_color = if selected { p.accent } else { fg };
    painter.text(Pos2::new(rect.left() + 22.0, y), egui::Align2::CENTER_CENTER, icon, body(17.0), icon_color);
    let mut label_width = rect.width() - 50.0;
    if let Some(badge) = badge {
        let area = painter.text(Pos2::new(rect.right() - 10.0, y), egui::Align2::RIGHT_CENTER, badge, body(12.0), p.faint);
        label_width -= area.width() + 8.0;
    }
    let font = if selected { semibold(14.0) } else { body(14.0) };
    paint_text(ui, painter, Pos2::new(rect.left() + 40.0, y), label, font, fg, label_width);
    response.clicked()
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        while let Some(result) = self.jobs.poll() {
            self.handle_result(&ctx, result);
        }
        if !self.pending_roots.is_empty() {
            let roots = std::mem::take(&mut self.pending_roots);
            self.start_scan(roots);
        }
        self.sync_appearance(&ctx);
        self.handle_shortcuts(&ctx);
        self.handle_drops(&ctx);

        self.sidebar(ui);
        self.top_bar(ui);
        self.progress_strip(ui);
        self.selection_bar(ui);
        self.show_inspector(ui);

        let p = pal(ui);
        egui::CentralPanel::no_frame().show(ui, |ui| {
            Frame::new().fill(p.bg).inner_margin(Margin { left: 20, right: 20, top: 6, bottom: 12 }).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                crate::pages::show(ui, self);
            });
        });

        crate::dialogs::show_review(&ctx, self);
        crate::dialogs::show_palette(&ctx, self);
        crate::dialogs::show_toasts(&ctx, self);
        crate::dialogs::show_drop_overlay(&ctx);

        if self.settings_dirty {
            self.settings_dirty = false;
            if let Err(err) = self.settings.save(&self.paths) {
                log::warn!("parametres non enregistres : {err:#}");
            }
            ctx.set_theme(theme_preference(self.settings.theme));
        }
        self.process_actions();

        #[cfg(feature = "shots")]
        crate::pages::shots::tick(&ctx, self);
    }
}

pub fn theme_preference(theme: Theme) -> ThemePreference {
    match theme {
        Theme::System => ThemePreference::System,
        Theme::Dark => ThemePreference::Dark,
        Theme::Light => ThemePreference::Light,
    }
}

fn open_path(path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .creation_flags(0x0800_0000)
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

fn reveal(path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(path);
        let _ = std::process::Command::new("explorer").raw_arg(arg).spawn();
    }
    #[cfg(not(windows))]
    {
        let folder = path.parent().unwrap_or(path);
        let _ = std::process::Command::new("xdg-open").arg(folder).spawn();
    }
}
