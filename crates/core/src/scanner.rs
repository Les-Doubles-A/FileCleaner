//! Scan recursif parallele. Sous Windows, `read_dir` fournit les metadonnees
//! sans appel systeme supplementaire, le parcours tient donc en une passe.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rayon::prelude::*;

use crate::config::{MISC_CATEGORY, QUARANTINE_DIRNAME, Settings};
use crate::model::{DirId, DirNode, FileEntry, NO_DIR, Progress, Scan, flags};
use crate::util::{is_within, now_ts, path_key};

const SKIPPED_DIR_NAMES: &[&str] = &[QUARANTINE_DIRNAME, "$recycle.bin", "system volume information"];

struct RawFile {
    name: Box<str>,
    size: u64,
    modified: i64,
    created: i64,
    accessed: i64,
    flags: u8,
}

struct RawDir {
    path: PathBuf,
    files: Vec<RawFile>,
    subdirs: Vec<RawDir>,
    modified: i64,
    hidden_entries: bool,
}

struct WalkCtx<'a> {
    excluded: Vec<PathBuf>,
    ignored_exts: Vec<String>,
    skip_hidden: bool,
    min_size: u64,
    max_size: u64,
    progress: &'a Progress,
    errors: Mutex<Vec<String>>,
}

fn unix(time: std::io::Result<SystemTime>) -> i64 {
    time.ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

#[cfg(windows)]
fn entry_flags(meta: &fs::Metadata) -> u8 {
    use std::os::windows::fs::MetadataExt;
    const READONLY: u32 = 0x1;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    const OFFLINE: u32 = 0x1000;
    const RECALL_ON_OPEN: u32 = 0x4_0000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
    let attrs = meta.file_attributes();
    let mut out = 0;
    if attrs & HIDDEN != 0 {
        out |= flags::HIDDEN;
    }
    if attrs & SYSTEM != 0 {
        out |= flags::SYSTEM;
    }
    if attrs & READONLY != 0 {
        out |= flags::READONLY;
    }
    if attrs & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0 {
        out |= flags::CLOUD;
    }
    out
}

#[cfg(not(windows))]
fn entry_flags(_meta: &fs::Metadata) -> u8 {
    0
}

fn extension_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(0) | None => "",
        Some(i) => &name[i + 1..],
    }
}

fn walk(path: PathBuf, ctx: &WalkCtx<'_>) -> RawDir {
    let mut raw = RawDir {
        modified: fs::metadata(&path).map_or(0, |m| unix(m.modified())),
        path,
        files: Vec::new(),
        subdirs: Vec::new(),
        hidden_entries: false,
    };
    if ctx.progress.is_cancelled() {
        return raw;
    }
    let reader = match fs::read_dir(&raw.path) {
        Ok(r) => r,
        Err(err) => {
            raw.hidden_entries = true;
            if let Ok(mut errors) = ctx.errors.lock() {
                errors.push(format!("{} : {err}", raw.path.display()));
            }
            return raw;
        }
    };

    let mut subdirs = Vec::new();
    for entry in reader {
        let Ok(entry) = entry else {
            raw.hidden_entries = true;
            continue;
        };
        let Ok(file_type) = entry.file_type() else {
            raw.hidden_entries = true;
            continue;
        };
        // Liens symboliques et jonctions ne sont jamais suivis.
        if file_type.is_symlink() {
            raw.hidden_entries = true;
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            raw.hidden_entries = true;
            continue;
        };
        let entry_flags = entry_flags(&meta);
        if ctx.skip_hidden && entry_flags & (flags::HIDDEN | flags::SYSTEM) != 0 {
            raw.hidden_entries = true;
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if file_type.is_dir() {
            let lower = name.to_lowercase();
            let child = entry.path();
            if SKIPPED_DIR_NAMES.contains(&lower.as_str())
                || ctx.excluded.iter().any(|e| is_within(&child, e))
            {
                raw.hidden_entries = true;
                continue;
            }
            subdirs.push(child);
        } else {
            let size = meta.len();
            let ext = extension_of(&name).to_lowercase();
            let filtered = size < ctx.min_size
                || (ctx.max_size > 0 && size > ctx.max_size)
                || ctx.ignored_exts.contains(&ext);
            if filtered {
                raw.hidden_entries = true;
                continue;
            }
            raw.files.push(RawFile {
                name: name.into_boxed_str(),
                size,
                modified: unix(meta.modified()),
                created: unix(meta.created()),
                accessed: unix(meta.accessed()),
                flags: entry_flags,
            });
            ctx.progress.bytes.fetch_add(size, Ordering::Relaxed);
        }
    }
    let n = ctx.progress.done.fetch_add(raw.files.len() as u64, Ordering::Relaxed);
    if n % 512 < raw.files.len() as u64 {
        ctx.progress.set_current(&raw.path.to_string_lossy());
    }
    raw.subdirs = subdirs.into_par_iter().map(|d| walk(d, ctx)).collect();
    raw
}

struct Flattener {
    dirs: Vec<DirNode>,
    files: Vec<FileEntry>,
    exts: Vec<Box<str>>,
    ext_ids: HashMap<String, u32>,
    ext_categories: HashMap<String, u8>,
    misc: u8,
}

impl Flattener {
    fn add(&mut self, raw: RawDir, parent: DirId) -> DirId {
        let id = self.dirs.len() as DirId;
        let name = raw
            .path
            .file_name()
            .map_or_else(|| raw.path.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
        let start = self.files.len() as u32;
        let mut size = 0;
        let count = raw.files.len() as u32;
        self.files.reserve(raw.files.len());
        for f in raw.files {
            let ext_lower = extension_of(&f.name).to_lowercase();
            let category = self.ext_categories.get(&ext_lower).copied().unwrap_or(self.misc);
            let ext = match self.ext_ids.get(&ext_lower) {
                Some(&e) => e,
                None => {
                    let e = self.exts.len() as u32;
                    self.exts.push(ext_lower.as_str().into());
                    self.ext_ids.insert(ext_lower, e);
                    e
                }
            };
            size += f.size;
            self.files.push(FileEntry {
                dir: id,
                name: f.name,
                ext,
                category,
                flags: f.flags,
                size: f.size,
                modified: f.modified,
                created: f.created,
                accessed: f.accessed,
            });
        }
        let end = self.files.len() as u32;
        self.dirs.push(DirNode {
            parent,
            path: raw.path,
            name: name.into_boxed_str(),
            children: Vec::with_capacity(raw.subdirs.len()),
            files: start..end,
            size,
            file_count: count,
            modified: raw.modified,
            hidden_entries: raw.hidden_entries,
            deleted: false,
        });
        for sub in raw.subdirs {
            let child = self.add(sub, id);
            let (child_size, child_count) = {
                let c = &self.dirs[child as usize];
                (c.size, c.file_count)
            };
            let node = &mut self.dirs[id as usize];
            node.children.push(child);
            node.size += child_size;
            node.file_count += child_count;
        }
        let mut children = std::mem::take(&mut self.dirs[id as usize].children);
        children.sort_by_key(|&c| std::cmp::Reverse(self.dirs[c as usize].size));
        self.dirs[id as usize].children = children;
        id
    }
}

/// Retire les doublons et les racines imbriquees dans une autre.
pub fn normalize_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut sorted: Vec<PathBuf> = roots.to_vec();
    sorted.sort_by_key(|p| path_key(p).len());
    let mut out: Vec<PathBuf> = Vec::new();
    for root in sorted {
        if !out.iter().any(|kept| is_within(&root, kept)) {
            out.push(root);
        }
    }
    out
}

pub fn scan(roots: &[PathBuf], settings: &Settings, progress: &Progress) -> Scan {
    let started = Instant::now();
    let roots = normalize_roots(roots);
    let ctx = WalkCtx {
        excluded: settings.excluded_folders.iter().map(PathBuf::from).collect(),
        ignored_exts: settings
            .ignored_extensions
            .iter()
            .map(|e| e.trim().trim_start_matches('.').to_lowercase())
            .filter(|e| !e.is_empty())
            .collect(),
        skip_hidden: settings.skip_hidden,
        min_size: settings.min_size_bytes,
        max_size: settings.max_size_bytes,
        progress,
        errors: Mutex::new(Vec::new()),
    };

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(settings.effective_threads())
        .stack_size(16 * 1024 * 1024)
        .build();
    let run = || roots.par_iter().map(|r| walk(r.clone(), &ctx)).collect::<Vec<_>>();
    let raw_roots = match pool {
        Ok(pool) => pool.install(run),
        Err(_) => run(),
    };

    let mut category_names: Vec<String> = Vec::new();
    let mut ext_categories = HashMap::new();
    for def in settings.categories.iter().filter(|c| c.enabled && c.name != MISC_CATEGORY) {
        let id = category_names.len() as u8;
        category_names.push(def.name.clone());
        for ext in &def.exts {
            ext_categories.insert(ext.trim_start_matches('.').to_lowercase(), id);
        }
    }
    let misc = category_names.len() as u8;
    category_names.push(MISC_CATEGORY.to_string());

    let mut flat = Flattener {
        dirs: Vec::new(),
        files: Vec::new(),
        exts: Vec::new(),
        ext_ids: HashMap::new(),
        ext_categories,
        misc,
    };
    let root_dirs = raw_roots.into_iter().map(|r| flat.add(r, NO_DIR)).collect();

    let cancelled = progress.is_cancelled();
    let errors = ctx.errors.into_inner().unwrap_or_default();
    log::info!(
        "scan: {} fichiers, {} dossiers, {} erreurs en {:?}",
        flat.files.len(),
        flat.dirs.len(),
        errors.len(),
        started.elapsed()
    );
    Scan {
        roots,
        root_dirs,
        dirs: flat.dirs,
        files: flat.files,
        exts: flat.exts,
        category_names,
        errors,
        cancelled,
        scanned_at: now_ts(),
        duration_ms: started.elapsed().as_millis() as u64,
        generation: 1,
    }
}

/// Vrai si `path` existe et est un dossier exploitable.
pub fn is_scannable(path: &Path) -> bool {
    path.is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::create_dir_all(root.join("empty/nested")).unwrap();
        fs::create_dir_all(root.join(QUARANTINE_DIRNAME)).unwrap();
        fs::write(root.join("a/photo.JPG"), vec![0u8; 100]).unwrap();
        fs::write(root.join("a/b/doc.pdf"), vec![0u8; 50]).unwrap();
        fs::write(root.join("noext"), b"x").unwrap();
        fs::write(root.join(".profile"), b"xy").unwrap();
        fs::write(root.join(QUARANTINE_DIRNAME).join("old.txt"), b"z").unwrap();
        dir
    }

    #[test]
    fn totals_and_categories() {
        let dir = tree();
        let scan = scan(&[dir.path().to_path_buf()], &Settings::default(), &Progress::default());
        assert_eq!(scan.total_files(), 4);
        assert_eq!(scan.total_size(), 153);
        let photo = scan.alive().find(|(_, f)| &*f.name == "photo.JPG").unwrap().1;
        assert_eq!(scan.ext_of(photo), "jpg");
        assert_eq!(scan.category_of(photo), "Images");
        let dotfile = scan.alive().find(|(_, f)| &*f.name == ".profile").unwrap().1;
        assert_eq!(scan.ext_of(dotfile), "");
    }

    #[test]
    fn quarantine_is_never_scanned() {
        let dir = tree();
        let scan = scan(&[dir.path().to_path_buf()], &Settings::default(), &Progress::default());
        assert!(scan.alive().all(|(_, f)| &*f.name != "old.txt"));
    }

    #[test]
    fn deleting_updates_ancestor_totals() {
        let dir = tree();
        let mut scan = scan(&[dir.path().to_path_buf()], &Settings::default(), &Progress::default());
        let id = scan.alive().find(|(_, f)| &*f.name == "doc.pdf").unwrap().0;
        scan.mark_files_deleted(&[id].into_iter().collect());
        assert_eq!(scan.total_size(), 103);
        assert_eq!(scan.total_files(), 3);
    }

    #[test]
    fn filters_flag_folders_as_not_empty() {
        let dir = tree();
        let settings = Settings { ignored_extensions: vec![".pdf".into()], ..Settings::default() };
        let scan = scan(&[dir.path().to_path_buf()], &settings, &Progress::default());
        let b = scan.dirs.iter().find(|d| &*d.name == "b").unwrap();
        assert_eq!(b.file_count, 0);
        assert!(b.hidden_entries);
    }

    #[test]
    fn nested_roots_collapse() {
        let roots = normalize_roots(&[PathBuf::from(r"C:\a\b"), PathBuf::from(r"C:\a"), PathBuf::from(r"C:\ab")]);
        assert_eq!(roots.len(), 2);
    }

    #[test]
    fn cancel_is_reported() {
        let dir = tree();
        let progress = Progress::default();
        progress.cancel();
        assert!(scan(&[dir.path().to_path_buf()], &Settings::default(), &progress).cancelled);
    }
}
