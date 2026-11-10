//! Structures de donnees centrales. Les fichiers et dossiers sont indexes par
//! des `u32` stables pour rester compacts sur des millions d'entrees.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde::Serialize;

pub type FileId = u32;
pub type DirId = u32;
pub const NO_DIR: DirId = u32::MAX;

pub mod flags {
    pub const HIDDEN: u8 = 1;
    pub const SYSTEM: u8 = 2;
    pub const READONLY: u8 = 4;
    /// Fichier cloud non present localement : le lire le telechargerait.
    pub const CLOUD: u8 = 8;
    pub const DELETED: u8 = 128;
}

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub dir: DirId,
    pub name: Box<str>,
    pub ext: u32,
    pub category: u8,
    pub flags: u8,
    pub size: u64,
    pub modified: i64,
    pub created: i64,
    pub accessed: i64,
}

impl FileEntry {
    pub fn is_alive(&self) -> bool {
        self.flags & flags::DELETED == 0
    }
    pub fn has(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }
}

#[derive(Clone, Debug)]
pub struct DirNode {
    pub parent: DirId,
    pub path: PathBuf,
    pub name: Box<str>,
    pub children: Vec<DirId>,
    /// Fichiers directs, contigus dans `Scan::files`.
    pub files: Range<u32>,
    /// Taille recursive.
    pub size: u64,
    /// Nombre recursif de fichiers.
    pub file_count: u32,
    pub modified: i64,
    /// Entrees non indexees (filtres, liens, erreurs) : le dossier n'est donc pas vide.
    pub hidden_entries: bool,
    pub deleted: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct CategoryInfo {
    pub name: String,
    pub count: u64,
    pub size: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExtInfo {
    pub ext: String,
    pub count: u64,
    pub size: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Scan {
    pub roots: Vec<PathBuf>,
    pub root_dirs: Vec<DirId>,
    pub dirs: Vec<DirNode>,
    pub files: Vec<FileEntry>,
    pub exts: Vec<Box<str>>,
    pub category_names: Vec<String>,
    pub errors: Vec<String>,
    pub cancelled: bool,
    pub scanned_at: i64,
    pub duration_ms: u64,
    /// Incremente a chaque modification : invalide les analyses derivees.
    pub generation: u64,
}

impl Scan {
    pub fn alive(&self) -> impl Iterator<Item = (FileId, &FileEntry)> + '_ {
        self.files.iter().enumerate().filter(|(_, f)| f.is_alive()).map(|(i, f)| (i as FileId, f))
    }

    pub fn file(&self, id: FileId) -> &FileEntry {
        &self.files[id as usize]
    }

    pub fn dir(&self, id: DirId) -> &DirNode {
        &self.dirs[id as usize]
    }

    pub fn file_path(&self, id: FileId) -> PathBuf {
        let file = self.file(id);
        self.dir(file.dir).path.join(&*file.name)
    }

    pub fn folder_of(&self, id: FileId) -> &Path {
        &self.dir(self.file(id).dir).path
    }

    pub fn ext_of(&self, file: &FileEntry) -> &str {
        &self.exts[file.ext as usize]
    }

    pub fn category_of(&self, file: &FileEntry) -> &str {
        &self.category_names[file.category as usize]
    }

    pub fn total_size(&self) -> u64 {
        self.root_dirs.iter().map(|&d| self.dir(d).size).sum()
    }

    pub fn total_files(&self) -> u64 {
        self.root_dirs.iter().map(|&d| u64::from(self.dir(d).file_count)).sum()
    }

    pub fn total_dirs(&self) -> u64 {
        self.dirs.iter().filter(|d| !d.deleted).count() as u64
    }

    pub fn is_root(&self, id: DirId) -> bool {
        self.dir(id).parent == NO_DIR
    }

    /// Libelle court de la cible d'analyse.
    pub fn title(&self) -> String {
        match self.roots.as_slice() {
            [] => String::new(),
            [one] => one.display().to_string(),
            many => format!("{} (+{})", many[0].display(), many.len() - 1),
        }
    }

    pub fn category_breakdown(&self) -> Vec<CategoryInfo> {
        let mut out: Vec<CategoryInfo> = self
            .category_names
            .iter()
            .map(|name| CategoryInfo { name: name.clone(), count: 0, size: 0 })
            .collect();
        for (_, f) in self.alive() {
            let c = &mut out[f.category as usize];
            c.count += 1;
            c.size += f.size;
        }
        out.retain(|c| c.count > 0);
        out.sort_by_key(|c| std::cmp::Reverse(c.size));
        out
    }

    pub fn extension_breakdown(&self) -> Vec<ExtInfo> {
        let mut stats = vec![(0u64, 0u64); self.exts.len()];
        for (_, f) in self.alive() {
            let s = &mut stats[f.ext as usize];
            s.0 += 1;
            s.1 += f.size;
        }
        let mut out: Vec<ExtInfo> = stats
            .into_iter()
            .enumerate()
            .filter(|(_, s)| s.0 > 0)
            .map(|(i, (count, size))| ExtInfo { ext: self.exts[i].to_string(), count, size })
            .collect();
        out.sort_by_key(|e| std::cmp::Reverse(e.size));
        out
    }

    /// Marque des fichiers comme supprimes et met a jour les totaux des dossiers.
    pub fn mark_files_deleted(&mut self, ids: &HashSet<FileId>) {
        for &id in ids {
            let (dir, size) = {
                let f = &mut self.files[id as usize];
                if !f.is_alive() {
                    continue;
                }
                f.flags |= flags::DELETED;
                (f.dir, f.size)
            };
            self.subtract_from_ancestors(dir, size, 1);
        }
        self.generation += 1;
    }

    /// Marque un sous-arbre complet comme supprime.
    pub fn mark_dir_deleted(&mut self, id: DirId) {
        let (size, count, parent) = {
            let d = self.dir(id);
            (d.size, d.file_count, d.parent)
        };
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            let node = &mut self.dirs[current as usize];
            node.deleted = true;
            node.size = 0;
            node.file_count = 0;
            let range = node.files.clone();
            stack.extend(node.children.iter().copied());
            for f in &mut self.files[range.start as usize..range.end as usize] {
                f.flags |= flags::DELETED;
            }
        }
        if parent != NO_DIR {
            self.subtract_from_ancestors(parent, size, count);
            self.dirs[parent as usize].children.retain(|&c| c != id);
        }
        self.generation += 1;
    }

    /// Applique un renommage deja effectue sur le disque.
    pub fn apply_renames(&mut self, renames: &HashMap<FileId, String>) {
        for (&id, name) in renames {
            self.files[id as usize].name = name.as_str().into();
        }
        self.generation += 1;
    }

    fn subtract_from_ancestors(&mut self, mut dir: DirId, size: u64, count: u32) {
        while dir != NO_DIR {
            let node = &mut self.dirs[dir as usize];
            node.size = node.size.saturating_sub(size);
            node.file_count = node.file_count.saturating_sub(count);
            dir = node.parent;
        }
    }

    /// Fichiers d'un sous-arbre (recursif).
    pub fn files_under(&self, id: DirId) -> Vec<FileId> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            let node = self.dir(current);
            out.extend(node.files.clone().filter(|&f| self.file(f).is_alive()));
            stack.extend(node.children.iter().copied());
        }
        out
    }
}

/// Avancement partage entre un travail de fond et l'interface.
#[derive(Debug, Default)]
pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub bytes: AtomicU64,
    pub cancelled: AtomicBool,
    pub current: Mutex<String>,
    pub phase: Mutex<String>,
}

impl Progress {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
    pub fn set_phase(&self, phase: &str) {
        if let Ok(mut p) = self.phase.lock() {
            phase.clone_into(&mut p);
        }
    }
    pub fn set_current(&self, value: &str) {
        if let Ok(mut c) = self.current.try_lock() {
            value.clone_into(&mut c);
        }
    }
    pub fn snapshot(&self) -> (u64, u64, u64, String, String) {
        (
            self.done.load(Ordering::Relaxed),
            self.total.load(Ordering::Relaxed),
            self.bytes.load(Ordering::Relaxed),
            self.phase.lock().map(|p| p.clone()).unwrap_or_default(),
            self.current.lock().map(|c| c.clone()).unwrap_or_default(),
        )
    }
}
