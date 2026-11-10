//! Journal persistant des operations et annulation.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::actions::move_path;
use crate::config::write_atomic;
use crate::util::now_ts;

const MAX_SESSIONS: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpKind {
    Trash,
    Quarantine,
    Permanent,
    Move,
    Rename,
    RemoveDir,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Operation {
    pub kind: OpKind,
    pub source: PathBuf,
    /// Emplacement apres l'operation (quarantaine, deplacement, renommage).
    pub destination: Option<PathBuf>,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub is_dir: bool,
    #[serde(default)]
    pub restored: bool,
    /// Supprime definitivement de la quarantaine.
    #[serde(default)]
    pub purged: bool,
}

impl Operation {
    pub fn is_restorable(&self) -> bool {
        !self.restored && !self.purged && self.kind != OpKind::Permanent
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub timestamp: i64,
    pub label: String,
    pub root: String,
    pub operations: Vec<Operation>,
}

impl Session {
    pub fn new(label: &str, root: &str) -> Self {
        let timestamp = now_ts();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        Self {
            id: format!("{timestamp:x}{unique:x}"),
            timestamp,
            label: label.to_string(),
            root: root.to_string(),
            operations: Vec::new(),
        }
    }

    pub fn total_bytes(&self) -> u64 {
        self.operations.iter().map(|o| o.size).sum()
    }

    pub fn restorable_count(&self) -> usize {
        self.operations.iter().filter(|o| o.is_restorable()).count()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct UndoReport {
    pub restored: usize,
    pub failed: usize,
}

#[derive(Debug, Default)]
pub struct History {
    pub sessions: Vec<Session>,
    file: PathBuf,
}

impl History {
    pub fn load(file: &Path) -> Self {
        let sessions = fs::read_to_string(file)
            .ok()
            .and_then(|text| {
                serde_json::from_str(&text)
                    .inspect_err(|err| {
                        log::warn!("historique illisible : {err}");
                        let _ = fs::copy(file, file.with_extension("json.bad"));
                    })
                    .ok()
            })
            .unwrap_or_default();
        Self { sessions, file: file.to_path_buf() }
    }

    pub fn save(&self) -> Result<()> {
        write_atomic(&self.file, serde_json::to_string(&self.sessions)?.as_bytes())
    }

    /// Enregistre une session terminee, les vides sont ignorees.
    pub fn commit(&mut self, session: Session) -> Result<()> {
        if session.operations.is_empty() {
            return Ok(());
        }
        self.sessions.insert(0, session);
        self.sessions.truncate(MAX_SESSIONS);
        self.save()
    }

    pub fn last_restorable(&self) -> Option<&Session> {
        self.sessions.iter().find(|s| s.restorable_count() > 0)
    }

    /// Operations en quarantaine encore presentes.
    pub fn quarantined(&self) -> impl Iterator<Item = (&Session, &Operation)> {
        self.sessions.iter().flat_map(|s| {
            s.operations
                .iter()
                .filter(|o| o.kind == OpKind::Quarantine && !o.restored && !o.purged)
                .map(move |o| (s, o))
        })
    }

    pub fn quarantine_bytes(&self) -> u64 {
        self.quarantined().map(|(_, o)| o.size).sum()
    }

    /// Annule une session, operations les plus recentes d'abord.
    pub fn undo(&mut self, session_id: &str) -> UndoReport {
        let mut report = UndoReport::default();
        let Some(session) = self.sessions.iter_mut().find(|s| s.id == session_id) else {
            return report;
        };
        let trash_ops: Vec<usize> = session
            .operations
            .iter()
            .enumerate()
            .filter(|(_, o)| o.kind == OpKind::Trash && o.is_restorable())
            .map(|(i, _)| i)
            .collect();
        if !trash_ops.is_empty() {
            let paths: Vec<PathBuf> =
                trash_ops.iter().map(|&i| session.operations[i].source.clone()).collect();
            let restored = restore_from_trash(&paths);
            for (&i, ok) in trash_ops.iter().zip(restored) {
                if ok {
                    session.operations[i].restored = true;
                    report.restored += 1;
                } else {
                    report.failed += 1;
                }
            }
        }
        for op in session.operations.iter_mut().rev() {
            if !op.is_restorable() || op.kind == OpKind::Trash {
                continue;
            }
            match undo_one(op) {
                Ok(()) => {
                    op.restored = true;
                    report.restored += 1;
                }
                Err(err) => {
                    log::warn!("restauration impossible {} : {err:#}", op.source.display());
                    report.failed += 1;
                }
            }
        }
        if let Err(err) = self.save() {
            log::warn!("historique non enregistre : {err:#}");
        }
        report
    }

    /// Restaure une seule operation de quarantaine.
    pub fn restore_quarantined(&mut self, session_id: &str, source: &Path) -> Result<()> {
        let op = self
            .sessions
            .iter_mut()
            .find(|s| s.id == session_id)
            .and_then(|s| s.operations.iter_mut().find(|o| o.source == source))
            .ok_or_else(|| anyhow::anyhow!("operation introuvable"))?;
        undo_one(op)?;
        op.restored = true;
        self.save()
    }

    /// Supprime definitivement des elements de quarantaine. Retourne les octets liberes.
    pub fn purge_quarantine(&mut self, older_than_days: Option<u32>) -> u64 {
        let limit = older_than_days.map(|d| now_ts() - i64::from(d) * 86_400);
        let mut freed = 0;
        for session in &mut self.sessions {
            if limit.is_some_and(|l| session.timestamp > l) {
                continue;
            }
            for op in &mut session.operations {
                if op.kind != OpKind::Quarantine || op.restored || op.purged {
                    continue;
                }
                let Some(path) = &op.destination else { continue };
                let removed = if op.is_dir { fs::remove_dir_all(path) } else { fs::remove_file(path) };
                match removed {
                    Ok(()) => {
                        op.purged = true;
                        freed += op.size;
                        if let Some(parent) = path.parent() {
                            let _ = fs::remove_dir(parent);
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => op.purged = true,
                    Err(err) => log::warn!("purge impossible {} : {err}", path.display()),
                }
            }
        }
        if let Err(err) = self.save() {
            log::warn!("historique non enregistre : {err:#}");
        }
        freed
    }

    pub fn clear(&mut self) -> Result<()> {
        self.sessions.retain(|s| {
            s.operations.iter().any(|o| o.kind == OpKind::Quarantine && !o.restored && !o.purged)
        });
        self.save()
    }
}

fn undo_one(op: &Operation) -> Result<()> {
    match op.kind {
        OpKind::RemoveDir => {
            fs::create_dir_all(&op.source)?;
            Ok(())
        }
        OpKind::Quarantine | OpKind::Move | OpKind::Rename => {
            let from = op.destination.as_ref().ok_or_else(|| anyhow::anyhow!("destination inconnue"))?;
            anyhow::ensure!(from.exists(), "{} n'existe plus", from.display());
            // Ne jamais ecraser un fichier revenu entre-temps.
            anyhow::ensure!(!op.source.exists(), "{} existe deja", op.source.display());
            if let Some(parent) = op.source.parent() {
                fs::create_dir_all(parent)?;
            }
            move_path(from, &op.source)?;
            if let Some(parent) = from.parent()
                && op.kind == OpKind::Quarantine {
                    let _ = fs::remove_dir(parent);
                }
            Ok(())
        }
        OpKind::Trash | OpKind::Permanent => anyhow::bail!("operation non restaurable ici"),
    }
}

/// Restaure depuis la corbeille. Retourne un booleen par chemin demande.
#[cfg(any(windows, all(unix, not(target_os = "macos"))))]
pub fn restore_from_trash(paths: &[PathBuf]) -> Vec<bool> {
    use crate::util::path_key;
    use std::collections::HashMap;

    let mut items = match trash::os_limited::list() {
        Ok(items) => items,
        Err(err) => {
            log::warn!("lecture de la corbeille impossible : {err}");
            return vec![false; paths.len()];
        }
    };
    // Le plus recent d'abord si un meme chemin a ete supprime plusieurs fois.
    items.sort_by_key(|i| std::cmp::Reverse(i.time_deleted));
    let mut by_path: HashMap<String, trash::TrashItem> = HashMap::new();
    for item in items {
        by_path.entry(path_key(&item.original_path())).or_insert(item);
    }
    paths
        .iter()
        .map(|path| {
            if path.exists() {
                return false;
            }
            let Some(item) = by_path.remove(&path_key(&long_form(path))) else {
                return false;
            };
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            trash::os_limited::restore_all([item])
                .inspect_err(|err| log::warn!("restauration corbeille : {err}"))
                .is_ok()
        })
        .collect()
}

/// La corbeille memorise le chemin long : un chemin 8.3 (`ADAM_E~1`) ne correspondrait pas.
#[cfg(any(windows, all(unix, not(target_os = "macos"))))]
fn long_form(path: &Path) -> PathBuf {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return path.to_path_buf();
    };
    match fs::canonicalize(parent) {
        Ok(full) => {
            let text = full.to_string_lossy();
            PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text)).join(name)
        }
        Err(_) => path.to_path_buf(),
    }
}

#[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
pub fn restore_from_trash(paths: &[PathBuf]) -> Vec<bool> {
    vec![false; paths.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(kind: OpKind, source: &Path, destination: Option<&Path>) -> Operation {
        Operation {
            kind,
            source: source.to_path_buf(),
            destination: destination.map(Path::to_path_buf),
            size: 3,
            is_dir: false,
            restored: false,
            purged: false,
        }
    }

    #[test]
    fn undo_moves_files_back_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("sub/a.txt"), dir.path().join("b.txt"));
        fs::write(&b, b"abc").unwrap();
        let file = dir.path().join("h.json");
        let mut history = History::load(&file);
        let mut session = Session::new("test", "root");
        session.operations.push(op(OpKind::Move, &a, Some(&b)));
        let id = session.id.clone();
        history.commit(session).unwrap();

        assert_eq!(history.undo(&id), UndoReport { restored: 1, failed: 0 });
        assert!(a.exists() && !b.exists());
        // Une seconde annulation ne fait rien.
        assert_eq!(history.undo(&id), UndoReport::default());
        assert!(History::load(&file).sessions[0].operations[0].restored);
    }

    #[test]
    fn undo_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"old").unwrap();
        let mut history = History::load(&dir.path().join("h.json"));
        let mut session = Session::new("t", "r");
        session.operations.push(op(OpKind::Rename, &a, Some(&b)));
        let id = session.id.clone();
        history.commit(session).unwrap();
        assert_eq!(history.undo(&id).failed, 1);
        assert_eq!(fs::read(&a).unwrap(), b"new");
    }

    #[test]
    fn purge_respects_age_and_marks_ops() {
        let dir = tempfile::tempdir().unwrap();
        let q = dir.path().join("q.bin");
        fs::write(&q, b"abc").unwrap();
        let mut history = History::load(&dir.path().join("h.json"));
        let mut session = Session::new("t", "r");
        session.operations.push(op(OpKind::Quarantine, &dir.path().join("orig"), Some(&q)));
        history.commit(session).unwrap();

        assert_eq!(history.purge_quarantine(Some(30)), 0, "session trop recente");
        assert!(q.exists());
        assert_eq!(history.purge_quarantine(None), 3);
        assert!(!q.exists());
        assert_eq!(history.quarantined().count(), 0);
    }

    #[test]
    fn corrupt_history_is_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("h.json");
        fs::write(&file, b"{oops").unwrap();
        assert!(History::load(&file).sessions.is_empty());
        assert!(file.with_extension("json.bad").exists());
    }
}
