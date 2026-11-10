//! Detection de doublons en trois etages : taille, empreinte partielle
//! (debut + fin), puis BLAKE3 complet. Un cache persistant evite de relire
//! les fichiers inchanges d'une analyse a l'autre.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::Ordering;

use rayon::prelude::*;
use same_file::Handle;
use serde::{Deserialize, Serialize};

use crate::config::write_atomic;
use crate::model::{FileId, Progress, Scan, flags};
use crate::util::{Label, is_within};

const SAMPLE: usize = 16 * 1024;
const MAX_CACHE_ENTRIES: usize = 2_000_000;

#[derive(Clone, Debug)]
pub struct DupGroup {
    /// Empreinte BLAKE3 (hexa), ou nom de fichier en mode `ByName`.
    pub key: String,
    pub size: u64,
    pub files: Vec<FileId>,
    /// Nombre de copies physiques : les liens durs ne liberent rien.
    pub physical_copies: usize,
    pub identical: bool,
}

impl DupGroup {
    pub fn recoverable(&self) -> u64 {
        self.size * (self.physical_copies.saturating_sub(1)) as u64
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DupMode {
    Content,
    ByName,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeepStrategy {
    Oldest,
    Newest,
    ShortestPath,
    LongestPath,
    InFolder(PathBuf),
}

impl KeepStrategy {
    pub fn label(&self) -> Label {
        match self {
            Self::Oldest => Label::new("Garder le plus ancien", "Keep the oldest"),
            Self::Newest => Label::new("Garder le plus récent", "Keep the newest"),
            Self::ShortestPath => Label::new("Garder le chemin le plus court", "Keep the shortest path"),
            Self::LongestPath => Label::new("Garder le chemin le plus long", "Keep the longest path"),
            Self::InFolder(_) => Label::new("Garder dans un dossier précis", "Keep in a given folder"),
        }
    }
}

/// Fichier a conserver dans un groupe selon la strategie.
pub fn pick_keeper(scan: &Scan, group: &DupGroup, strategy: &KeepStrategy) -> FileId {
    let path_len = |id: &FileId| scan.file_path(*id).as_os_str().len();
    let by_age = |id: &FileId| scan.file(*id).modified;
    let found = match strategy {
        KeepStrategy::Oldest => group.files.iter().copied().min_by_key(|id| by_age(id)),
        KeepStrategy::Newest => group.files.iter().copied().max_by_key(|id| by_age(id)),
        KeepStrategy::ShortestPath => group.files.iter().copied().min_by_key(|id| path_len(id)),
        KeepStrategy::LongestPath => group.files.iter().copied().max_by_key(|id| path_len(id)),
        KeepStrategy::InFolder(folder) => group
            .files
            .iter()
            .copied()
            .find(|id| is_within(scan.folder_of(*id), folder))
            .or_else(|| group.files.iter().copied().min_by_key(|id| by_age(id))),
    };
    found.unwrap_or(group.files[0])
}

/// Fichiers a retirer pour ne garder qu'un exemplaire de chaque groupe identique.
/// Les groupes par nom sont ignores : leur contenu peut differer.
pub fn select_redundant(scan: &Scan, groups: &[DupGroup], strategy: &KeepStrategy) -> Vec<FileId> {
    let mut out = Vec::new();
    for group in groups.iter().filter(|g| g.identical) {
        let keeper = pick_keeper(scan, group, strategy);
        out.extend(group.files.iter().copied().filter(|&id| id != keeper));
    }
    out
}

/// Groupes dont tous les exemplaires sont selectionnes (perte de donnees).
pub fn fully_selected(groups: &[DupGroup], selection: &HashSet<FileId>) -> usize {
    groups
        .iter()
        .filter(|g| g.identical && g.files.iter().all(|f| selection.contains(f)))
        .count()
}

/// Fichier candidat et son empreinte complete, si elle est deja connue.
type Candidate = (FileId, Option<[u8; 32]>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CacheEntry {
    size: u64,
    modified: i64,
    partial: u64,
    full: Option<[u8; 32]>,
}

#[derive(Debug, Default)]
pub struct HashCache {
    entries: HashMap<PathBuf, CacheEntry>,
    dirty: bool,
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

impl HashCache {
    pub fn load(path: &Path) -> Self {
        let mut cache = Self::default();
        let Ok(text) = fs::read_to_string(path) else {
            return cache;
        };
        for line in text.lines() {
            let mut parts = line.splitn(5, '\t');
            let (Some(size), Some(modified), Some(partial), Some(full), Some(file)) =
                (parts.next(), parts.next(), parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let (Ok(size), Ok(modified), Ok(partial)) =
                (size.parse(), modified.parse(), u64::from_str_radix(partial, 16))
            else {
                continue;
            };
            cache.entries.insert(
                PathBuf::from(file),
                CacheEntry { size, modified, partial, full: from_hex(full) },
            );
        }
        cache
    }

    pub fn save(&mut self, path: &Path) {
        if !self.dirty {
            return;
        }
        let mut text = String::with_capacity(self.entries.len() * 160);
        for (file, e) in self.entries.iter().take(MAX_CACHE_ENTRIES) {
            let full = e.full.map(|h| to_hex(&h)).unwrap_or_default();
            text.push_str(&format!(
                "{}\t{}\t{:x}\t{}\t{}\n",
                e.size,
                e.modified,
                e.partial,
                full,
                file.display()
            ));
        }
        match write_atomic(path, text.as_bytes()) {
            Ok(()) => self.dirty = false,
            Err(err) => log::warn!("cache d'empreintes non enregistre : {err:#}"),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn get(&self, path: &Path, size: u64, modified: i64) -> Option<CacheEntry> {
        self.entries.get(path).copied().filter(|e| e.size == size && e.modified == modified)
    }
}

fn partial_hash(path: &Path, size: u64) -> std::io::Result<u64> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0u8; SAMPLE];
    let n = file.read(&mut buffer)?;
    hasher.update(&buffer[..n]);
    if size > (SAMPLE * 2) as u64 {
        file.seek(SeekFrom::End(-(SAMPLE as i64)))?;
        let n = file.read(&mut buffer)?;
        hasher.update(&buffer[..n]);
    }
    let digest = hasher.finalize();
    let mut head = [0u8; 8];
    head.copy_from_slice(&digest.as_bytes()[..8]);
    Ok(u64::from_le_bytes(head))
}

fn full_hash(path: &Path, progress: &Progress) -> std::io::Result<Option<[u8; 32]>> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0u8; 512 * 1024];
    loop {
        if progress.is_cancelled() {
            return Ok(None);
        }
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        progress.done.fetch_add(n as u64, Ordering::Relaxed);
    }
    Ok(Some(*hasher.finalize().as_bytes()))
}

/// Compte les copies physiques en ecartant les liens durs vers le meme fichier.
fn physical_copies(scan: &Scan, files: &[FileId]) -> usize {
    let mut seen = HashSet::new();
    let mut unknown = 0;
    for &id in files {
        match Handle::from_path(scan.file_path(id)) {
            Ok(handle) => {
                seen.insert(handle);
            }
            Err(_) => unknown += 1,
        }
    }
    seen.len() + unknown
}

pub fn find_by_content(
    scan: &Scan,
    min_size: u64,
    cache: &Mutex<HashCache>,
    progress: &Progress,
) -> Vec<DupGroup> {
    let min_size = min_size.max(1);
    let mut by_size: HashMap<u64, Vec<FileId>> = HashMap::new();
    for (id, f) in scan.alive() {
        // Lire un fichier cloud absent du disque declencherait son telechargement.
        if f.size >= min_size && !f.has(flags::CLOUD) {
            by_size.entry(f.size).or_default().push(id);
        }
    }
    let candidates: Vec<FileId> =
        by_size.into_values().filter(|g| g.len() > 1).flatten().collect();

    // Etage 2 : empreinte partielle.
    progress.set_phase("partial");
    progress.done.store(0, Ordering::Relaxed);
    progress.total.store(candidates.len() as u64, Ordering::Relaxed);
    let cached: Vec<Option<CacheEntry>> = {
        let guard = cache.lock().expect("cache");
        candidates
            .iter()
            .map(|&id| {
                let f = scan.file(id);
                guard.get(&scan.file_path(id), f.size, f.modified)
            })
            .collect()
    };
    let partials: Vec<(FileId, u64, Option<[u8; 32]>)> = candidates
        .par_iter()
        .zip(cached.par_iter())
        .filter_map(|(&id, entry)| {
            if progress.is_cancelled() {
                return None;
            }
            progress.done.fetch_add(1, Ordering::Relaxed);
            if let Some(e) = entry {
                return Some((id, e.partial, e.full));
            }
            let path = scan.file_path(id);
            match partial_hash(&path, scan.file(id).size) {
                Ok(hash) => Some((id, hash, None)),
                Err(err) => {
                    log::debug!("empreinte partielle impossible {}: {err}", path.display());
                    None
                }
            }
        })
        .collect();
    if progress.is_cancelled() {
        return Vec::new();
    }

    let mut by_partial: HashMap<(u64, u64), Vec<Candidate>> = HashMap::new();
    for &(id, partial, full) in &partials {
        by_partial.entry((scan.file(id).size, partial)).or_default().push((id, full));
    }
    let stage3: Vec<Candidate> =
        by_partial.into_values().filter(|g| g.len() > 1).flatten().collect();

    // Etage 3 : empreinte complete, avancement exprime en octets.
    progress.set_phase("full");
    progress.done.store(0, Ordering::Relaxed);
    let to_read: u64 = stage3.iter().filter(|(_, h)| h.is_none()).map(|(id, _)| scan.file(*id).size).sum();
    progress.total.store(to_read, Ordering::Relaxed);
    let hashed: Vec<(FileId, [u8; 32])> = stage3
        .par_iter()
        .filter_map(|&(id, known)| {
            if let Some(hash) = known {
                return Some((id, hash));
            }
            let path = scan.file_path(id);
            progress.set_current(&path.to_string_lossy());
            match full_hash(&path, progress) {
                Ok(hash) => hash.map(|h| (id, h)),
                Err(err) => {
                    log::debug!("empreinte impossible {}: {err}", path.display());
                    None
                }
            }
        })
        .collect();

    {
        let mut guard = cache.lock().expect("cache");
        let full_by_id: HashMap<FileId, [u8; 32]> = hashed.iter().copied().collect();
        for &(id, partial, _) in &partials {
            let f = scan.file(id);
            guard.entries.insert(
                scan.file_path(id),
                CacheEntry {
                    size: f.size,
                    modified: f.modified,
                    partial,
                    full: full_by_id.get(&id).copied(),
                },
            );
        }
        guard.dirty = true;
    }
    if progress.is_cancelled() {
        return Vec::new();
    }

    let mut by_hash: HashMap<[u8; 32], Vec<FileId>> = HashMap::new();
    for (id, hash) in hashed {
        by_hash.entry(hash).or_default().push(id);
    }
    let mut groups: Vec<DupGroup> = by_hash
        .into_par_iter()
        .filter(|(_, files)| files.len() > 1)
        .map(|(hash, files)| {
            let physical_copies = physical_copies(scan, &files);
            DupGroup {
                key: to_hex(&hash),
                size: scan.file(files[0]).size,
                files,
                physical_copies,
                identical: true,
            }
        })
        .filter(|g| g.physical_copies > 1)
        .collect();
    groups.sort_by_key(|g| std::cmp::Reverse(g.recoverable()));
    groups
}

/// Fichiers portant le meme nom dans des dossiers differents.
pub fn find_by_name(scan: &Scan) -> Vec<DupGroup> {
    let mut by_name: HashMap<String, Vec<FileId>> = HashMap::new();
    for (id, f) in scan.alive() {
        by_name.entry(f.name.to_lowercase()).or_default().push(id);
    }
    let mut groups: Vec<DupGroup> = by_name
        .into_iter()
        .filter(|(_, files)| files.len() > 1)
        .map(|(key, files)| {
            let size = files.iter().map(|&f| scan.file(f).size).max().unwrap_or(0);
            let physical_copies = files.len();
            DupGroup { key, size, files, physical_copies, identical: false }
        })
        .collect();
    groups.sort_by_key(|g| std::cmp::Reverse(g.files.len() as u64 * g.size));
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::scanner::scan;

    fn scan_of(dir: &Path) -> Scan {
        scan(&[dir.to_path_buf()], &Settings::default(), &Progress::default())
    }

    fn find(scan: &Scan) -> Vec<DupGroup> {
        find_by_content(scan, 1, &Mutex::new(HashCache::default()), &Progress::default())
    }

    #[test]
    fn same_size_different_tail_is_not_a_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = vec![7u8; 100_000];
        let b = a.clone();
        fs::write(dir.path().join("a.bin"), &a).unwrap();
        fs::write(dir.path().join("b.bin"), &b).unwrap();
        a[50_000] = 8; // Difference hors des zones echantillonnees.
        fs::write(dir.path().join("c.bin"), &a).unwrap();
        let s = scan_of(dir.path());
        let groups = find(&s);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].files.len(), 2);
        assert_eq!(groups[0].recoverable(), 100_000);
    }

    #[test]
    fn hard_links_free_nothing() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.bin"), b"hello world").unwrap();
        fs::hard_link(dir.path().join("a.bin"), dir.path().join("b.bin")).unwrap();
        assert!(find(&scan_of(dir.path())).is_empty());
    }

    #[test]
    fn empty_files_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a"), b"").unwrap();
        fs::write(dir.path().join("b"), b"").unwrap();
        assert!(find(&scan_of(dir.path())).is_empty());
    }

    #[test]
    fn strategy_always_keeps_one_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("a.txt"), b"same").unwrap();
        fs::write(dir.path().join("sub/b.txt"), b"same").unwrap();
        fs::write(dir.path().join("sub/c.txt"), b"same").unwrap();
        let s = scan_of(dir.path());
        let groups = find(&s);
        let remove = select_redundant(&s, &groups, &KeepStrategy::ShortestPath);
        assert_eq!(remove.len(), 2);
        assert!(remove.iter().all(|&id| &*s.file(id).name != "a.txt"));
        let keep_sub = KeepStrategy::InFolder(dir.path().join("sub"));
        let remove = select_redundant(&s, &groups, &keep_sub);
        assert!(remove.iter().any(|&id| &*s.file(id).name == "a.txt"));
    }

    #[test]
    fn name_groups_never_produce_automatic_removals() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("notes.txt"), b"version A").unwrap();
        fs::write(dir.path().join("sub/notes.txt"), b"version B !").unwrap();
        let s = scan_of(dir.path());
        let groups = find_by_name(&s);
        assert_eq!(groups.len(), 1);
        assert!(select_redundant(&s, &groups, &KeepStrategy::Oldest).is_empty());
    }

    #[test]
    fn full_selection_is_detected() {
        let group = DupGroup { key: "k".into(), size: 1, files: vec![1, 2], physical_copies: 2, identical: true };
        assert_eq!(fully_selected(std::slice::from_ref(&group), &[1].into_iter().collect()), 0);
        assert_eq!(fully_selected(&[group], &[1, 2].into_iter().collect()), 1);
    }

    #[test]
    fn cache_roundtrip_skips_rereading() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"same").unwrap();
        fs::write(dir.path().join("b.txt"), b"same").unwrap();
        let s = scan_of(dir.path());
        let cache_file = dir.path().join("cache.tsv");
        let cache = Mutex::new(HashCache::default());
        assert_eq!(find_by_content(&s, 1, &cache, &Progress::default()).len(), 1);
        cache.lock().unwrap().save(&cache_file);

        let reloaded = Mutex::new(HashCache::load(&cache_file));
        assert_eq!(reloaded.lock().unwrap().len(), 2);
        let progress = Progress::default();
        assert_eq!(find_by_content(&s, 1, &reloaded, &progress).len(), 1);
        assert_eq!(progress.total.load(Ordering::Relaxed), 0, "aucun octet relu");
    }
}
