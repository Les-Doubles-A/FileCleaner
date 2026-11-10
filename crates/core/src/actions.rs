//! Execution des actions sur le disque. Chaque action produit une session
//! d'historique ; rien n'est ecrase, et un fichier modifie depuis l'analyse
//! n'est pas touche.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::{QUARANTINE_DIRNAME, Settings};
use crate::history::{OpKind, Operation, Session};
use crate::model::{DirId, FileId, Progress, Scan};
use crate::safety::protection_reason;
use crate::util::{Label, Lang, unique_path};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DeleteMode {
    #[default]
    Trash,
    Quarantine,
    Permanent,
}

impl DeleteMode {
    pub fn label(self) -> Label {
        match self {
            Self::Trash => Label::new("Corbeille Windows", "Recycle Bin"),
            Self::Quarantine => Label::new("Quarantaine locale", "Local quarantine"),
            Self::Permanent => Label::new("Suppression définitive", "Permanent deletion"),
        }
    }
}

/// Element a supprimer, avec l'etat attendu sur le disque.
#[derive(Clone, Debug)]
pub struct Target {
    pub path: PathBuf,
    pub size: u64,
    pub is_dir: bool,
    /// Date de modification attendue pour un fichier ; `None` = pas de verification.
    pub expected_modified: Option<i64>,
    pub file: Option<FileId>,
    pub dir: Option<DirId>,
}

impl Target {
    pub fn from_file(scan: &Scan, id: FileId) -> Self {
        let file = scan.file(id);
        Self {
            path: scan.file_path(id),
            size: file.size,
            is_dir: false,
            expected_modified: Some(file.modified),
            file: Some(id),
            dir: None,
        }
    }

    pub fn from_dir(scan: &Scan, id: DirId) -> Self {
        let dir = scan.dir(id);
        Self {
            path: dir.path.clone(),
            size: dir.size,
            is_dir: true,
            expected_modified: None,
            file: None,
            dir: Some(id),
        }
    }

    pub fn from_path(path: PathBuf, size: u64, is_dir: bool) -> Self {
        Self { path, size, is_dir, expected_modified: None, file: None, dir: None }
    }
}

#[derive(Debug, Default)]
pub struct ActionReport {
    pub session: Option<Session>,
    pub succeeded: usize,
    pub failed: Vec<(PathBuf, String)>,
    pub freed_bytes: u64,
    pub done_files: Vec<FileId>,
    pub done_dirs: Vec<DirId>,
}

/// Deplacement avec repli copie + suppression entre volumes.
pub fn move_path(from: &Path, to: &Path) -> Result<()> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::CrossesDevices => {
            if from.is_dir() {
                copy_dir(from, to)?;
                fs::remove_dir_all(from).with_context(|| format!("suppression de {}", from.display()))
            } else {
                let copied = fs::copy(from, to).with_context(|| format!("copie vers {}", to.display()))?;
                let expected = fs::metadata(from)?.len();
                if copied != expected {
                    let _ = fs::remove_file(to);
                    anyhow::bail!("copie incomplete de {}", from.display());
                }
                fs::remove_file(from).with_context(|| format!("suppression de {}", from.display()))
            }
        }
        Err(err) => Err(err).with_context(|| format!("deplacement de {}", from.display())),
    }
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn modified_unix(meta: &fs::Metadata) -> Option<i64> {
    meta.modified().ok()?.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}

/// Refuse une cible protegee, disparue ou modifiee depuis l'analyse.
fn precheck(target: &Target, lang: Lang) -> Result<(), String> {
    if let Some(reason) = protection_reason(&target.path) {
        return Err(reason.get(lang).to_string());
    }
    let meta = fs::symlink_metadata(&target.path).map_err(|err| err.to_string())?;
    if let Some(expected) = target.expected_modified
        && (meta.len() != target.size || modified_unix(&meta) != Some(expected)) {
            return Err(match lang {
                Lang::Fr => "Modifié depuis l'analyse".into(),
                Lang::En => "Changed since the scan".into(),
            });
        }
    Ok(())
}

fn quarantine_root(settings: &Settings, target: &Path, session_id: &str, roots: &[PathBuf]) -> PathBuf {
    let base = match &settings.quarantine_folder {
        Some(folder) if !folder.trim().is_empty() => PathBuf::from(folder),
        _ => roots
            .iter()
            .find(|r| crate::util::is_within(target, r))
            .or(roots.first())
            .cloned()
            .or_else(|| target.parent().map(Path::to_path_buf))
            .unwrap_or_default()
            .join(QUARANTINE_DIRNAME),
    };
    base.join(session_id)
}

/// Marque le dossier de quarantaine comme cache, sans effet s'il l'est deja.
#[cfg(windows)]
fn hide(path: &Path) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new("attrib")
        .arg("+h")
        .arg(path)
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

#[cfg(not(windows))]
fn hide(_path: &Path) {}

pub struct DeleteRequest<'a> {
    pub label: &'a str,
    pub root: &'a str,
    pub roots: &'a [PathBuf],
    pub mode: DeleteMode,
    pub lang: Lang,
}

const TRASH_BATCH: usize = 48;

pub fn delete_targets(
    targets: &[Target],
    request: &DeleteRequest<'_>,
    settings: &Settings,
    progress: &Progress,
) -> ActionReport {
    let mut session = Session::new(request.label, request.root);
    let mut report = ActionReport::default();
    progress.done.store(0, Ordering::Relaxed);
    progress.total.store(targets.len() as u64, Ordering::Relaxed);

    let mut ready: Vec<&Target> = Vec::with_capacity(targets.len());
    for target in targets {
        match precheck(target, request.lang) {
            Ok(()) => ready.push(target),
            Err(message) => {
                report.failed.push((target.path.clone(), message));
                progress.done.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    let record = |report: &mut ActionReport, session: &mut Session, target: &Target, kind, destination| {
        session.operations.push(Operation {
            kind,
            source: target.path.clone(),
            destination,
            size: target.size,
            is_dir: target.is_dir,
            restored: false,
            purged: false,
        });
        report.succeeded += 1;
        report.freed_bytes += target.size;
        report.done_files.extend(target.file);
        report.done_dirs.extend(target.dir);
    };

    match request.mode {
        DeleteMode::Trash => {
            for batch in ready.chunks(TRASH_BATCH) {
                if progress.is_cancelled() {
                    break;
                }
                progress.set_current(&batch[0].path.to_string_lossy());
                let result = trash::delete_all(batch.iter().map(|t| &t.path));
                for target in batch {
                    let gone = !target.path.exists();
                    let outcome = if gone {
                        Ok(())
                    } else if result.is_err() {
                        // Le lot a echoue : l'erreur individuelle dit quel fichier bloque.
                        trash::delete(&target.path).map_err(|err| err.to_string())
                    } else {
                        Err("Toujours présent après suppression".to_string())
                    };
                    match outcome {
                        Ok(()) => record(&mut report, &mut session, target, OpKind::Trash, None),
                        Err(message) => report.failed.push((target.path.clone(), message)),
                    }
                    progress.done.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        DeleteMode::Quarantine => {
            let mut hidden = std::collections::HashSet::new();
            for target in ready {
                if progress.is_cancelled() {
                    break;
                }
                progress.set_current(&target.path.to_string_lossy());
                let folder = quarantine_root(settings, &target.path, &session.id, request.roots);
                let outcome = fs::create_dir_all(&folder)
                    .context("creation de la quarantaine")
                    .and_then(|()| {
                        if let Some(parent) = folder.parent().filter(|p| hidden.insert(p.to_path_buf())) {
                            hide(parent);
                        }
                        let name = target.path.file_name().unwrap_or_default();
                        let destination = unique_path(&folder.join(name));
                        move_path(&target.path, &destination).map(|()| destination)
                    });
                match outcome {
                    Ok(destination) => {
                        record(&mut report, &mut session, target, OpKind::Quarantine, Some(destination));
                    }
                    Err(err) => report.failed.push((target.path.clone(), format!("{err:#}"))),
                }
                progress.done.fetch_add(1, Ordering::Relaxed);
            }
        }
        DeleteMode::Permanent => {
            for target in ready {
                if progress.is_cancelled() {
                    break;
                }
                progress.set_current(&target.path.to_string_lossy());
                let outcome = if target.is_dir {
                    fs::remove_dir_all(&target.path)
                } else {
                    fs::remove_file(&target.path)
                };
                match outcome {
                    Ok(()) => record(&mut report, &mut session, target, OpKind::Permanent, None),
                    Err(err) => report.failed.push((target.path.clone(), err.to_string())),
                }
                progress.done.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    report.session = Some(session);
    report
}

/// Supprime des dossiers vides, du plus profond au plus haut, sans jamais
/// retirer de contenu : `remove_dir` echoue si un fichier est apparu.
pub fn remove_empty_dirs(scan: &Scan, dirs: &[DirId], label: &str, lang: Lang) -> ActionReport {
    let mut session = Session::new(label, &scan.title());
    let mut report = ActionReport::default();
    for &top in dirs {
        let mut ordered = Vec::new();
        let mut stack = vec![top];
        while let Some(id) = stack.pop() {
            ordered.push(id);
            stack.extend(scan.dir(id).children.iter().copied());
        }
        let mut complete = true;
        for &id in ordered.iter().rev() {
            let path = &scan.dir(id).path;
            if let Some(reason) = protection_reason(path) {
                report.failed.push((path.clone(), reason.get(lang).to_string()));
                complete = false;
                continue;
            }
            match fs::remove_dir(path) {
                Ok(()) => {
                    session.operations.push(Operation {
                        kind: OpKind::RemoveDir,
                        source: path.clone(),
                        destination: None,
                        size: 0,
                        is_dir: true,
                        restored: false,
                        purged: false,
                    });
                    report.succeeded += 1;
                }
                Err(err) => {
                    report.failed.push((path.clone(), err.to_string()));
                    complete = false;
                }
            }
        }
        if complete {
            report.done_dirs.push(top);
        }
    }
    report.session = Some(session);
    report
}

/// Deplace une liste de fichiers `(source, destination)` sans jamais ecraser.
pub fn move_files(moves: &[(PathBuf, PathBuf)], label: &str, root: &str, progress: &Progress) -> ActionReport {
    let mut session = Session::new(label, root);
    let mut report = ActionReport::default();
    progress.done.store(0, Ordering::Relaxed);
    progress.total.store(moves.len() as u64, Ordering::Relaxed);
    for (source, destination) in moves {
        if progress.is_cancelled() {
            break;
        }
        progress.set_current(&source.to_string_lossy());
        let outcome = (|| -> Result<(PathBuf, u64)> {
            let size = fs::metadata(source)?.len();
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            let destination = unique_path(destination);
            move_path(source, &destination)?;
            Ok((destination, size))
        })();
        match outcome {
            Ok((destination, size)) => {
                session.operations.push(Operation {
                    kind: OpKind::Move,
                    source: source.clone(),
                    destination: Some(destination),
                    size,
                    is_dir: false,
                    restored: false,
                    purged: false,
                });
                report.succeeded += 1;
            }
            Err(err) => report.failed.push((source.clone(), format!("{err:#}"))),
        }
        progress.done.fetch_add(1, Ordering::Relaxed);
    }
    report.session = Some(session);
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Progress;
    use crate::scanner::scan;

    fn request<'a>(mode: DeleteMode, roots: &'a [PathBuf]) -> DeleteRequest<'a> {
        DeleteRequest { label: "test", root: "root", roots, mode, lang: Lang::En }
    }

    #[test]
    fn quarantine_moves_and_records() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        fs::write(&file, b"data").unwrap();
        let roots = vec![dir.path().to_path_buf()];
        let s = scan(&roots, &Settings::default(), &Progress::default());
        let targets = [Target::from_file(&s, 0)];
        let report = delete_targets(
            &targets,
            &request(DeleteMode::Quarantine, &roots),
            &Settings::default(),
            &Progress::default(),
        );
        assert_eq!(report.succeeded, 1, "{:?}", report.failed);
        assert_eq!(report.freed_bytes, 4);
        assert!(!file.exists());
        let session = report.session.unwrap();
        let dest = session.operations[0].destination.as_ref().unwrap();
        assert!(dest.exists());
        assert!(dest.to_string_lossy().contains(QUARANTINE_DIRNAME));
    }

    #[test]
    fn modified_file_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        fs::write(&file, b"data").unwrap();
        let roots = vec![dir.path().to_path_buf()];
        let s = scan(&roots, &Settings::default(), &Progress::default());
        fs::write(&file, b"changed content").unwrap();
        let report = delete_targets(
            &[Target::from_file(&s, 0)],
            &request(DeleteMode::Permanent, &roots),
            &Settings::default(),
            &Progress::default(),
        );
        assert_eq!(report.succeeded, 0);
        assert_eq!(report.failed.len(), 1);
        assert!(file.exists());
    }

    #[cfg(windows)]
    #[test]
    fn protected_paths_are_refused() {
        let windir = PathBuf::from(std::env::var_os("SystemRoot").unwrap());
        let targets = [Target::from_path(windir.join("notepad.exe"), 0, false)];
        let report = delete_targets(
            &targets,
            &request(DeleteMode::Permanent, &[]),
            &Settings::default(),
            &Progress::default(),
        );
        assert_eq!(report.succeeded, 0);
        assert!(windir.join("notepad.exe").exists());
    }

    #[test]
    fn trash_then_restore_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let name = format!("fc-test-{}.txt", std::process::id());
        let file = dir.path().join(&name);
        fs::write(&file, b"trash me").unwrap();
        let roots = vec![dir.path().to_path_buf()];
        let s = scan(&roots, &Settings::default(), &Progress::default());
        let report = delete_targets(
            &[Target::from_file(&s, 0)],
            &request(DeleteMode::Trash, &roots),
            &Settings::default(),
            &Progress::default(),
        );
        assert_eq!(report.succeeded, 1, "{:?}", report.failed);
        assert!(!file.exists());
        let restored = crate::history::restore_from_trash(std::slice::from_ref(&file));
        assert_eq!(restored, [true]);
        assert_eq!(fs::read(&file).unwrap(), b"trash me");
    }

    #[test]
    fn empty_dir_removal_refuses_new_content() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("e/nested")).unwrap();
        fs::create_dir_all(dir.path().join("f")).unwrap();
        let s = scan(&[dir.path().to_path_buf()], &Settings::default(), &Progress::default());
        let dirs = crate::analysis::find_empty_dirs(&s);
        assert_eq!(dirs.len(), 2);
        fs::write(dir.path().join("f/late.txt"), b"x").unwrap();
        let report = remove_empty_dirs(&s, &dirs, "t", Lang::En);
        assert!(!dir.path().join("e").exists());
        assert!(dir.path().join("f/late.txt").exists());
        assert_eq!(report.failed.len(), 1);
    }

    #[test]
    fn moves_never_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.txt"), dir.path().join("out/a.txt"));
        fs::create_dir_all(b.parent().unwrap()).unwrap();
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"old").unwrap();
        let report = move_files(&[(a.clone(), b.clone())], "t", "r", &Progress::default());
        assert_eq!(report.succeeded, 1);
        assert_eq!(fs::read(&b).unwrap(), b"old");
        assert_eq!(fs::read(dir.path().join("out/a (1).txt")).unwrap(), b"new");
    }
}
