//! Emplacements systeme a nettoyer, lecteurs et dossiers usuels.

use std::fs;
use std::path::{Path, PathBuf};

use directories::UserDirs;
use rayon::prelude::*;
use sysinfo::Disks;

use crate::actions::Target;
use crate::safety::protection_reason;
use crate::util::{Label, now_ts};

#[derive(Clone, Debug)]
pub struct SystemTarget {
    pub id: &'static str,
    pub label: Label,
    pub hint: Label,
    pub folders: Vec<PathBuf>,
    /// Ne retenir que les fichiers plus vieux que N heures (fichiers en cours d'usage).
    pub min_age_hours: u32,
    pub preselected: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Measure {
    pub size: u64,
    pub files: u64,
    /// Entrees directes retenues pour le nettoyage.
    pub entries: Vec<(PathBuf, u64, bool)>,
}

fn env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

/// Profils de navigateurs bases sur Chromium.
fn chromium_caches(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "Default" || name.starts_with("Profile ") {
            let profile = entry.path();
            out.push(profile.join("Cache"));
            out.push(profile.join("Code Cache"));
            out.push(profile.join("GPUCache"));
        }
    }
    out
}

pub fn system_targets() -> Vec<SystemTarget> {
    let local = env("LOCALAPPDATA").unwrap_or_default();
    let roaming = env("APPDATA").unwrap_or_default();
    let windir = env("SystemRoot").unwrap_or_else(|| PathBuf::from("C:\\Windows"));
    let user_temp = std::env::temp_dir();
    let mut firefox = Vec::new();
    if let Ok(entries) = fs::read_dir(local.join("Mozilla/Firefox/Profiles")) {
        firefox.extend(entries.flatten().map(|e| e.path().join("cache2")));
    }

    let targets = vec![
        SystemTarget {
            id: "temp",
            label: Label::new("Fichiers temporaires utilisateur", "User temporary files"),
            hint: Label::new("Dossier %TEMP%, fichiers de plus de 24 h", "%TEMP% folder, files older than 24 h"),
            folders: vec![user_temp],
            min_age_hours: 24,
            preselected: true,
        },
        SystemTarget {
            id: "windows-temp",
            label: Label::new("Fichiers temporaires Windows", "Windows temporary files"),
            hint: Label::new("Peut nécessiter les droits administrateur", "May require administrator rights"),
            folders: vec![windir.join("Temp")],
            min_age_hours: 24,
            preselected: false,
        },
        SystemTarget {
            id: "crash",
            label: Label::new("Rapports de plantage", "Crash reports"),
            hint: Label::new("CrashDumps et rapports d'erreurs Windows", "CrashDumps and Windows error reports"),
            folders: vec![
                local.join("CrashDumps"),
                local.join("Microsoft/Windows/WER/ReportArchive"),
                local.join("Microsoft/Windows/WER/ReportQueue"),
            ],
            min_age_hours: 0,
            preselected: true,
        },
        SystemTarget {
            id: "inet",
            label: Label::new("Cache Internet Windows", "Windows Internet cache"),
            hint: Label::new("INetCache, régénéré automatiquement", "INetCache, rebuilt automatically"),
            folders: vec![local.join("Microsoft/Windows/INetCache")],
            min_age_hours: 1,
            preselected: true,
        },
        SystemTarget {
            id: "chrome",
            label: Label::new("Cache Google Chrome", "Google Chrome cache"),
            hint: Label::new("Fermez Chrome pour tout libérer", "Close Chrome to free everything"),
            folders: chromium_caches(&local.join("Google/Chrome/User Data")),
            min_age_hours: 0,
            preselected: false,
        },
        SystemTarget {
            id: "edge",
            label: Label::new("Cache Microsoft Edge", "Microsoft Edge cache"),
            hint: Label::new("Fermez Edge pour tout libérer", "Close Edge to free everything"),
            folders: chromium_caches(&local.join("Microsoft/Edge/User Data")),
            min_age_hours: 0,
            preselected: false,
        },
        SystemTarget {
            id: "brave",
            label: Label::new("Cache Brave", "Brave cache"),
            hint: Label::new("Fermez Brave pour tout libérer", "Close Brave to free everything"),
            folders: chromium_caches(&local.join("BraveSoftware/Brave-Browser/User Data")),
            min_age_hours: 0,
            preselected: false,
        },
        SystemTarget {
            id: "firefox",
            label: Label::new("Cache Firefox", "Firefox cache"),
            hint: Label::new("Fermez Firefox pour tout libérer", "Close Firefox to free everything"),
            folders: firefox,
            min_age_hours: 0,
            preselected: false,
        },
        SystemTarget {
            id: "npm",
            label: Label::new("Cache npm", "npm cache"),
            hint: Label::new("Paquets retéléchargés à la demande", "Packages are downloaded again on demand"),
            folders: vec![local.join("npm-cache")],
            min_age_hours: 0,
            preselected: false,
        },
        SystemTarget {
            id: "pip",
            label: Label::new("Cache pip", "pip cache"),
            hint: Label::new("Paquets retéléchargés à la demande", "Packages are downloaded again on demand"),
            folders: vec![local.join("pip/Cache")],
            min_age_hours: 0,
            preselected: false,
        },
        SystemTarget {
            id: "discord",
            label: Label::new("Cache Discord", "Discord cache"),
            hint: Label::new("Médias mis en cache", "Cached media"),
            folders: vec![roaming.join("discord/Cache"), roaming.join("discord/Code Cache")],
            min_age_hours: 0,
            preselected: false,
        },
    ];
    targets.into_iter().filter(|t| t.folders.iter().any(|f| f.is_dir())).collect()
}

fn dir_totals(path: &Path) -> (u64, u64, i64) {
    let Ok(entries) = fs::read_dir(path) else {
        return (0, 0, 0);
    };
    let (mut size, mut files, mut newest) = (0, 0, 0);
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            subdirs.push(entry.path());
        } else if let Ok(meta) = entry.metadata() {
            size += meta.len();
            files += 1;
            newest = newest.max(modified(&meta));
        }
    }
    let nested: Vec<_> = subdirs.par_iter().map(|d| dir_totals(d)).collect();
    for (s, f, n) in nested {
        size += s;
        files += f;
        newest = newest.max(n);
    }
    (size, files, newest)
}

fn modified(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

/// Mesure une cible. Seules les entrees assez anciennes sont retenues : un dossier
/// contenant un fichier recent est conserve entier.
pub fn measure(target: &SystemTarget) -> Measure {
    let limit = now_ts() - i64::from(target.min_age_hours) * 3600;
    let mut out = Measure::default();
    for folder in &target.folders {
        let Ok(entries) = fs::read_dir(folder) else { continue };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            let (size, files, newest, is_dir) = if kind.is_dir() {
                let (s, f, n) = dir_totals(&path);
                (s, f, n, true)
            } else {
                let Ok(meta) = entry.metadata() else { continue };
                (meta.len(), 1, modified(&meta), false)
            };
            if target.min_age_hours > 0 && newest > limit {
                continue;
            }
            if protection_reason(&path).is_some() {
                continue;
            }
            out.size += size;
            out.files += files;
            out.entries.push((path, size, is_dir));
        }
    }
    out
}

impl Measure {
    pub fn targets(&self) -> Vec<Target> {
        self.entries.iter().map(|(p, s, d)| Target::from_path(p.clone(), *s, *d)).collect()
    }
}

/// Contenu de la corbeille : (elements, octets).
#[cfg(any(windows, all(unix, not(target_os = "macos"))))]
pub fn recycle_bin_stats() -> Option<(u64, u64)> {
    let items = trash::os_limited::list().ok()?;
    let mut bytes = 0;
    for item in &items {
        if let Ok(meta) = trash::os_limited::metadata(item) {
            bytes += meta.size.size().unwrap_or(0);
        }
    }
    Some((items.len() as u64, bytes))
}

#[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
pub fn recycle_bin_stats() -> Option<(u64, u64)> {
    None
}

/// Vide la corbeille (irreversible).
#[cfg(any(windows, all(unix, not(target_os = "macos"))))]
pub fn empty_recycle_bin() -> anyhow::Result<()> {
    let items = trash::os_limited::list()?;
    trash::os_limited::purge_all(items)?;
    Ok(())
}

#[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
pub fn empty_recycle_bin() -> anyhow::Result<()> {
    anyhow::bail!("non pris en charge sur ce systeme")
}

const CONTEXT_MENU_KEY: &str = r"HKCU\Software\Classes\Directory\shell\FileCleaner";

#[cfg(windows)]
fn reg(args: &[&str]) -> anyhow::Result<bool> {
    use std::os::windows::process::CommandExt;
    let status = std::process::Command::new("reg")
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x0800_0000)
        .status()?;
    Ok(status.success())
}

/// Vrai si l'entree du menu contextuel de l'Explorateur existe.
#[cfg(windows)]
pub fn context_menu_installed() -> bool {
    reg(&["query", CONTEXT_MENU_KEY]).unwrap_or(false)
}

/// Ajoute ou retire l'entree, pour l'utilisateur courant uniquement.
#[cfg(windows)]
pub fn set_context_menu(enabled: bool, exe: &Path, label: &str) -> anyhow::Result<()> {
    if !enabled {
        reg(&["delete", CONTEXT_MENU_KEY, "/f"])?;
        return Ok(());
    }
    let exe = exe.display().to_string();
    let command_key = format!(r"{CONTEXT_MENU_KEY}\command");
    let command = format!("\"{exe}\" \"%1\"");
    let ok = reg(&["add", CONTEXT_MENU_KEY, "/ve", "/d", label, "/f"])?
        && reg(&["add", CONTEXT_MENU_KEY, "/v", "Icon", "/d", &exe, "/f"])?
        && reg(&["add", &command_key, "/ve", "/d", &command, "/f"])?;
    anyhow::ensure!(ok, "ecriture du registre refusee");
    Ok(())
}

#[cfg(not(windows))]
pub fn context_menu_installed() -> bool {
    false
}

#[cfg(not(windows))]
pub fn set_context_menu(_enabled: bool, _exe: &Path, _label: &str) -> anyhow::Result<()> {
    anyhow::bail!("non pris en charge sur ce systeme")
}

#[derive(Clone, Debug)]
pub struct Drive {
    pub mount: PathBuf,
    pub name: String,
    pub total: u64,
    pub free: u64,
}

pub fn drives() -> Vec<Drive> {
    let Ok(disks) = Disks::new_with_refreshed_list() else {
        return Vec::new();
    };
    let mut out: Vec<Drive> = disks
        .iter()
        .map(|d| Drive {
            mount: d.mount_point().to_path_buf(),
            name: d.name().to_string_lossy().into_owned(),
            total: d.total_space(),
            free: d.available_space(),
        })
        .filter(|d| d.total > 0)
        .collect();
    out.sort_by(|a, b| a.mount.cmp(&b.mount));
    out
}

/// Dossiers usuels de l'utilisateur.
pub fn known_folders() -> Vec<(Label, PathBuf)> {
    let Some(dirs) = UserDirs::new() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut push = |label: Label, path: Option<&Path>| {
        if let Some(path) = path {
            out.push((label, path.to_path_buf()));
        }
    };
    push(Label::new("Téléchargements", "Downloads"), dirs.download_dir());
    push(Label::new("Bureau", "Desktop"), dirs.desktop_dir());
    push(Label::new("Documents", "Documents"), dirs.document_dir());
    push(Label::new("Images", "Pictures"), dirs.picture_dir());
    push(Label::new("Vidéos", "Videos"), dirs.video_dir());
    push(Label::new("Musique", "Music"), dirs.audio_dir());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measure_keeps_recent_entries_out_when_aged() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("fresh.tmp"), b"abc").unwrap();
        let target = SystemTarget {
            id: "t",
            label: Label::new("", ""),
            hint: Label::new("", ""),
            folders: vec![dir.path().to_path_buf()],
            min_age_hours: 24,
            preselected: false,
        };
        assert_eq!(measure(&target).size, 0);
        let no_age = SystemTarget { min_age_hours: 0, ..target };
        let m = measure(&no_age);
        assert_eq!((m.size, m.files, m.entries.len()), (3, 1, 1));
    }

    #[test]
    fn drives_are_listed() {
        assert!(!drives().is_empty());
    }
}
