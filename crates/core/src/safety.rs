//! Garde-fous : emplacements qu'aucune action ne doit toucher.

use std::path::{Path, PathBuf};

use crate::util::{Label, is_within, path_key};

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

/// Dossiers systeme dont le contenu n'est jamais modifiable depuis l'application.
fn protected_trees() -> Vec<PathBuf> {
    ["SystemRoot", "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "ProgramData"]
        .iter()
        .filter_map(|v| env_path(v))
        .collect()
}

/// Dossiers intouchables eux-memes, leur contenu restant libre.
fn protected_exact() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = ["USERPROFILE", "APPDATA", "LOCALAPPDATA", "PUBLIC", "SystemDrive"]
        .iter()
        .filter_map(|v| env_path(v))
        .collect();
    if let Some(profile) = env_path("USERPROFILE") {
        for name in ["Desktop", "Documents", "Downloads", "Pictures", "Music", "Videos", "OneDrive"] {
            out.push(profile.join(name));
        }
        if let Some(users) = profile.parent() {
            out.push(users.to_path_buf());
        }
    }
    out
}

/// Exceptions dans les arbres proteges (ex: `C:\Windows\Temp`).
fn allowed_inside_protected(path: &Path) -> bool {
    env_path("SystemRoot").is_some_and(|root| {
        ["Temp", "Prefetch", "SoftwareDistribution\\Download", "Logs\\CBS"]
            .iter()
            .any(|sub| {
                let allowed = root.join(sub);
                is_within(path, &allowed) && path_key(path) != path_key(&allowed)
            })
    })
}

/// Raison pour laquelle `path` ne doit pas etre modifie, le cas echeant.
pub fn protection_reason(path: &Path) -> Option<Label> {
    let is_drive_root = path.parent().is_none() || path_key(path).len() <= 3;
    if is_drive_root {
        return Some(Label::new("Racine de lecteur", "Drive root"));
    }
    if protected_exact().iter().any(|p| path_key(p) == path_key(path)) {
        return Some(Label::new("Dossier personnel essentiel", "Essential user folder"));
    }
    if protected_trees().iter().any(|p| is_within(path, p)) && !allowed_inside_protected(path) {
        return Some(Label::new("Dossier système protégé", "Protected system folder"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn system_locations_are_protected() {
        let windir = PathBuf::from(std::env::var_os("SystemRoot").unwrap());
        assert!(protection_reason(&windir).is_some());
        assert!(protection_reason(&windir.join("System32/kernel32.dll")).is_some());
        assert!(protection_reason(Path::new("C:\\")).is_some());
        assert!(protection_reason(&windir.join("Temp")).is_some());
        assert!(protection_reason(&windir.join("Temp/foo.tmp")).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn user_folders_are_protected_but_not_their_content() {
        let profile = PathBuf::from(std::env::var_os("USERPROFILE").unwrap());
        assert!(protection_reason(&profile).is_some());
        assert!(protection_reason(&profile.join("Downloads")).is_some());
        assert!(protection_reason(&profile.join("Downloads/setup.exe")).is_none());
    }
}
