//! Travaux de fond : l'interface ne se bloque jamais sur le disque.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, ColorImage};
use filecleaner_core::actions::ActionReport;
use filecleaner_core::analysis::{self, DevArtifact, Insights, JunkHit};
use filecleaner_core::config::Settings;
use filecleaner_core::duplicates::{DupGroup, DupMode, HashCache};
use filecleaner_core::history::UndoReport;
use filecleaner_core::model::{CategoryInfo, DirId, ExtInfo, FileId, Progress, Scan};
use filecleaner_core::rename::RenameReport;
use filecleaner_core::similar::SimilarGroup;
use filecleaner_core::system::Measure;

/// Listes derivees d'un scan, recalculees a chaque changement de generation.
#[derive(Default)]
pub struct Derived {
    pub generation: u64,
    pub junk: Vec<JunkHit>,
    pub junk_rules: HashMap<FileId, usize>,
    pub empty: Vec<DirId>,
    pub dev: Vec<DevArtifact>,
    pub insights: Insights,
    pub categories: Vec<CategoryInfo>,
    pub extensions: Vec<ExtInfo>,
}

impl Derived {
    pub fn compute(scan: &Scan, settings: &Settings) -> Self {
        let junk = analysis::find_junk(scan, settings);
        let empty = analysis::find_empty_dirs(scan);
        let dev = analysis::find_dev_artifacts(scan);
        let insights = Insights::from_parts(scan, settings, &junk, &dev, empty.len());
        Self {
            generation: scan.generation,
            junk_rules: junk.iter().map(|h| (h.file, h.rule)).collect(),
            junk,
            empty,
            dev,
            insights,
            categories: scan.category_breakdown(),
            extensions: scan.extension_breakdown(),
        }
    }
}

pub enum JobResult {
    Scan(Box<Scan>),
    Derived(Box<Derived>),
    Duplicates(Vec<DupGroup>, DupMode, u64),
    Similar(Vec<SimilarGroup>),
    Delete(ActionReport),
    EmptyDirs(ActionReport),
    Moved(ActionReport),
    Renamed(RenameReport),
    Undone(UndoReport),
    Measured(&'static str, Measure),
    RecycleBin(Option<(u64, u64)>),
    RecycleEmptied(Result<(), String>),
    Exported(Result<PathBuf, String>),
    PickedFolders(PickPurpose, Vec<PathBuf>),
    PickedSave(ExportFormat, Option<PathBuf>),
    Drives(Vec<filecleaner_core::system::Drive>),
    Thumbnail(PathBuf, Option<Arc<ColorImage>>),
    TextPreview(PathBuf, Option<String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickPurpose {
    Scan,
    AddRoot,
    Quarantine,
    Exclude,
    MoveSelection,
    KeepFolder,
    OrganizeDestination,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Json,
    Html,
    Pdf,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
            Self::Html => "html",
            Self::Pdf => "pdf",
        }
    }
}

/// Tache visible par l'utilisateur, une seule a la fois.
pub struct Running {
    pub title: String,
    pub progress: Arc<Progress>,
    pub cancellable: bool,
}

pub struct Jobs {
    tx: Sender<JobResult>,
    rx: Receiver<JobResult>,
    ctx: egui::Context,
    pub running: Option<Running>,
    pub hash_cache: Arc<Mutex<HashCache>>,
}

impl Jobs {
    pub fn new(ctx: egui::Context, hash_cache: HashCache) -> Self {
        let (tx, rx) = mpsc::channel();
        Self { tx, rx, ctx, running: None, hash_cache: Arc::new(Mutex::new(hash_cache)) }
    }

    pub fn poll(&self) -> Option<JobResult> {
        self.rx.try_recv().ok()
    }

    /// Lance un travail discret, sans indicateur.
    pub fn quiet(&self, work: impl FnOnce() -> JobResult + Send + 'static) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(work());
            ctx.request_repaint();
        });
    }

    /// Lance un travail avec barre d'avancement.
    pub fn start(
        &mut self,
        title: &str,
        cancellable: bool,
        work: impl FnOnce(&Progress) -> JobResult + Send + 'static,
    ) -> bool {
        if self.running.is_some() {
            return false;
        }
        let progress = Arc::new(Progress::default());
        self.running = Some(Running {
            title: title.to_string(),
            progress: progress.clone(),
            cancellable,
        });
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let result = work(&progress);
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        true
    }

    pub fn sender(&self) -> (Sender<JobResult>, egui::Context) {
        (self.tx.clone(), self.ctx.clone())
    }
}
