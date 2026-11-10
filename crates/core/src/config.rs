//! Parametres utilisateur et emplacements des donnees locales.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::actions::DeleteMode;
use crate::util::Lang;

pub const QUARANTINE_DIRNAME: &str = ".filecleaner_quarantine";

/// Repertoires de donnees. `FILECLEANER_HOME` permet de les isoler (tests, mode portable).
#[derive(Clone, Debug)]
pub struct AppPaths {
    pub config: PathBuf,
    pub data: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> Self {
        let paths = if let Some(home) = std::env::var_os("FILECLEANER_HOME") {
            let home = PathBuf::from(home);
            Self { config: home.clone(), data: home }
        } else if let Some(dirs) = ProjectDirs::from("", "", "FileCleaner") {
            Self { config: dirs.config_dir().to_path_buf(), data: dirs.data_local_dir().to_path_buf() }
        } else {
            let fallback = std::env::temp_dir().join("FileCleaner");
            Self { config: fallback.clone(), data: fallback }
        };
        let _ = fs::create_dir_all(&paths.config);
        let _ = fs::create_dir_all(&paths.data);
        paths
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.json")
    }
    pub fn history_file(&self) -> PathBuf {
        self.data.join("history.json")
    }
    pub fn snapshots_file(&self) -> PathBuf {
        self.data.join("snapshots.json")
    }
    pub fn hash_cache_file(&self) -> PathBuf {
        self.data.join("hash-cache.tsv")
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.data.join("logs")
    }
}

/// Ecriture atomique : fichier temporaire puis renommage.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).with_context(|| format!("ecriture de {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("remplacement de {}", path.display()))?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CategoryDef {
    pub name: String,
    pub exts: Vec<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

pub const MISC_CATEGORY: &str = "Divers";

/// Traduction des categories integrees ; une categorie utilisateur garde son nom.
pub fn category_label(name: &str, lang: Lang) -> &str {
    const TABLE: &[(&str, &str)] = &[
        ("Vidéos", "Videos"),
        ("Bases de données", "Databases"),
        ("Polices", "Fonts"),
        ("Modèles 3D", "3D models"),
        ("Divers", "Other"),
    ];
    if lang == Lang::En
        && let Some((_, en)) = TABLE.iter().find(|(fr, _)| *fr == name) {
            return en;
        }
    name
}

pub fn default_categories() -> Vec<CategoryDef> {
    let def = |name: &str, exts: &[&str]| CategoryDef {
        name: name.into(),
        exts: exts.iter().map(|s| (*s).to_string()).collect(),
        enabled: true,
    };
    vec![
        def("Images", &["jpg", "jpeg", "png", "gif", "bmp", "webp", "svg", "heic", "heif", "tif", "tiff", "ico", "raw", "cr2", "nef", "arw", "dng", "psd", "avif"]),
        def("Vidéos", &["mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "ts", "3gp"]),
        def("Audio", &["mp3", "wav", "flac", "aac", "ogg", "wma", "m4a", "opus", "aiff"]),
        def("Documents", &["doc", "docx", "odt", "txt", "rtf", "xls", "xlsx", "ods", "ppt", "pptx", "odp", "csv", "md", "tex", "pages", "numbers", "key"]),
        def("PDF", &["pdf"]),
        def("Archives", &["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "iso", "zst", "cab", "tgz"]),
        def("Code", &["py", "js", "jsx", "tsx", "html", "htm", "css", "scss", "java", "c", "cpp", "h", "hpp", "cs", "php", "rb", "go", "rs", "json", "xml", "yaml", "yml", "toml", "sh", "ps1", "kt", "swift", "lua", "vue"]),
        def("Applications", &["exe", "msi", "msix", "apk", "app", "dmg", "bat", "cmd", "deb", "rpm", "appimage", "dll"]),
        def("Bases de données", &["db", "sqlite", "sqlite3", "mdb", "accdb", "sql"]),
        def("Polices", &["ttf", "otf", "woff", "woff2", "fon"]),
        def("E-books", &["epub", "mobi", "azw", "azw3", "fb2", "djvu"]),
        def("Modèles 3D", &["obj", "fbx", "blend", "stl", "3ds", "gltf", "glb", "step", "stp"]),
    ]
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    // Apparence
    pub theme: Theme,
    pub language: Lang,
    pub accent: [u8; 3],
    pub ui_scale: f32,
    pub compact_rows: bool,

    // Filtres d'analyse
    pub excluded_folders: Vec<String>,
    pub ignored_extensions: Vec<String>,
    pub skip_hidden: bool,
    pub min_size_bytes: u64,
    /// 0 = illimite.
    pub max_size_bytes: u64,
    pub threads: usize,

    // Seuils
    pub big_file_threshold_mb: u64,
    pub old_file_threshold_days: u32,
    pub rare_ext_max: u32,
    pub similar_threshold: u32,
    pub min_duplicate_bytes: u64,
    pub auto_duplicates: bool,

    // Securite
    pub delete_mode: DeleteMode,
    pub quarantine_folder: Option<String>,
    /// 0 = jamais de purge automatique.
    pub quarantine_retention_days: u32,

    pub categories: Vec<CategoryDef>,
    pub custom_junk_patterns: Vec<String>,

    pub recent_folders: Vec<String>,
    pub organize_template: String,
    pub rename_template: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            language: Lang::Fr,
            accent: [64, 138, 255],
            ui_scale: 1.0,
            compact_rows: false,
            excluded_folders: Vec::new(),
            ignored_extensions: Vec::new(),
            skip_hidden: false,
            min_size_bytes: 0,
            max_size_bytes: 0,
            threads: 0,
            big_file_threshold_mb: 100,
            old_file_threshold_days: 730,
            rare_ext_max: 3,
            similar_threshold: 6,
            min_duplicate_bytes: 1,
            auto_duplicates: true,
            delete_mode: DeleteMode::Trash,
            quarantine_folder: None,
            quarantine_retention_days: 0,
            categories: default_categories(),
            custom_junk_patterns: Vec::new(),
            recent_folders: Vec::new(),
            organize_template: "{category}/{year}".into(),
            rename_template: "{name}".into(),
        }
    }
}

impl Settings {
    pub fn load(paths: &AppPaths) -> Self {
        let file = paths.settings_file();
        let mut settings = match fs::read_to_string(&file) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|err| {
                log::warn!("parametres illisibles ({err}), valeurs par defaut");
                let _ = fs::copy(&file, file.with_extension("json.bad"));
                Self::default()
            }),
            Err(_) => Self::default(),
        };
        if settings.categories.is_empty() {
            settings.categories = default_categories();
        }
        settings.ui_scale = settings.ui_scale.clamp(0.75, 2.0);
        settings
    }

    pub fn save(&self, paths: &AppPaths) -> Result<()> {
        write_atomic(&paths.settings_file(), serde_json::to_string_pretty(self)?.as_bytes())
    }

    pub fn push_recent(&mut self, folder: &str) {
        self.recent_folders.retain(|f| !f.eq_ignore_ascii_case(folder));
        self.recent_folders.insert(0, folder.to_string());
        self.recent_folders.truncate(8);
    }

    /// Vrai si la detection des fichiers inutiles doit etre recalculee.
    pub fn junk_related_changed(&self, other: &Self) -> bool {
        self.custom_junk_patterns != other.custom_junk_patterns
            || self.big_file_threshold_mb != other.big_file_threshold_mb
            || self.old_file_threshold_days != other.old_file_threshold_days
    }

    pub fn effective_threads(&self) -> usize {
        if self.threads > 0 {
            self.threads.min(64)
        } else {
            std::thread::available_parallelism().map_or(4, |n| n.get()).min(16)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_fields_and_missing_fields_are_tolerated() {
        let s: Settings = serde_json::from_str(r#"{"language":"en","bogus":1}"#).unwrap();
        assert_eq!(s.language, Lang::En);
        assert_eq!(s.big_file_threshold_mb, 100);
    }

    #[test]
    fn recents_are_deduplicated_and_capped() {
        let mut s = Settings::default();
        for i in 0..12 {
            s.push_recent(&format!("C:\\d{i}"));
        }
        s.push_recent("c:\\D11");
        assert_eq!(s.recent_folders.len(), 8);
        assert_eq!(s.recent_folders[0], "c:\\D11");
    }
}
