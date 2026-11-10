//! Utilitaires generiques : formatage, dates, noms de fichiers, chemins.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Local, TimeZone};
use serde::{Deserialize, Serialize};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    Fr,
    En,
}

/// Texte bilingue statique, utilise par le moteur pour ses libelles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Label {
    pub fr: &'static str,
    pub en: &'static str,
}

impl Label {
    pub const fn new(fr: &'static str, en: &'static str) -> Self {
        Self { fr, en }
    }
    pub fn get(&self, lang: Lang) -> &'static str {
        match lang {
            Lang::Fr => self.fr,
            Lang::En => self.en,
        }
    }
}

const UNITS_FR: [&str; 6] = ["o", "Ko", "Mo", "Go", "To", "Po"];
const UNITS_EN: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];

/// Formate une taille en octets, ex: `12,4 Mo` ou `12.4 MB`.
pub fn format_size(bytes: u64, lang: Lang) -> String {
    let units = if lang == Lang::Fr { &UNITS_FR } else { &UNITS_EN };
    if bytes < 1024 {
        return format!("{bytes} {}", units[0]);
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let text = if value >= 100.0 {
        format!("{value:.0}")
    } else if value >= 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    };
    let text = if lang == Lang::Fr { text.replace('.', ",") } else { text };
    format!("{text} {}", units[unit])
}

/// Nombre decimal avec le separateur de la langue.
pub fn format_decimal(value: f64, digits: usize, lang: Lang) -> String {
    let text = format!("{value:.digits$}");
    if lang == Lang::Fr { text.replace('.', ",") } else { text }
}

/// Formate un entier avec separateur de milliers.
pub fn format_count(n: u64, lang: Lang) -> String {
    let digits = n.to_string();
    let sep = if lang == Lang::Fr { '\u{202f}' } else { ',' };
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

pub fn now_ts() -> i64 {
    Local::now().timestamp()
}

pub fn local_time(ts: i64) -> Option<DateTime<Local>> {
    Local.timestamp_opt(ts, 0).single()
}

pub fn format_datetime(ts: i64, lang: Lang) -> String {
    if ts <= 0 {
        return "-".into();
    }
    match local_time(ts) {
        Some(dt) if lang == Lang::Fr => dt.format("%d/%m/%Y %H:%M").to_string(),
        Some(dt) => dt.format("%Y-%m-%d %H:%M").to_string(),
        None => "-".into(),
    }
}

pub fn format_date(ts: i64, lang: Lang) -> String {
    if ts <= 0 {
        return "-".into();
    }
    match local_time(ts) {
        Some(dt) if lang == Lang::Fr => dt.format("%d/%m/%Y").to_string(),
        Some(dt) => dt.format("%Y-%m-%d").to_string(),
        None => "-".into(),
    }
}

/// Decrit l'anciennete d'un horodatage, ex: `il y a 3 mois`.
pub fn format_age(ts: i64, lang: Lang) -> String {
    let secs = (now_ts() - ts).max(0);
    let (n, fr, en) = match secs {
        s if s < 60 => return if lang == Lang::Fr { "à l'instant".into() } else { "just now".into() },
        s if s < 3600 => (s / 60, "min", "min"),
        s if s < 86_400 => (s / 3600, "h", "h"),
        s if s < 86_400 * 30 => (s / 86_400, "j", "d"),
        s if s < 86_400 * 365 => (s / (86_400 * 30), "mois", "mo"),
        s if s < 86_400 * 730 => (1, "an", "y"),
        s => (s / (86_400 * 365), "ans", "y"),
    };
    match lang {
        Lang::Fr => format!("il y a {n} {fr}"),
        Lang::En => format!("{n} {en} ago"),
    }
}

pub fn days_since(ts: i64) -> i64 {
    ((now_ts() - ts) / 86_400).max(0)
}

pub fn year_month_day(ts: i64) -> (i32, u32, u32) {
    local_time(ts).map_or((1970, 1, 1), |d| (d.year(), d.month(), d.day()))
}

/// Retire les accents (NFD puis suppression des marques combinantes).
pub fn strip_accents(text: &str) -> String {
    text.nfd().filter(|c| !is_combining_mark(*c)).collect()
}

const INVALID_CHARS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Retire les caracteres interdits d'un nom de fichier.
pub fn sanitize_component(name: &str) -> String {
    name.chars()
        .filter(|c| !INVALID_CHARS.contains(c) && !c.is_control())
        .collect::<String>()
        .trim_matches(|c: char| c == ' ' || c == '.')
        .to_string()
}

/// Retourne la raison pour laquelle un nom est invalide, s'il l'est.
pub fn filename_problem(name: &str) -> Option<Label> {
    if name.trim().is_empty() {
        return Some(Label::new("Nom vide", "Empty name"));
    }
    if name.chars().any(|c| INVALID_CHARS.contains(&c) || c.is_control()) {
        return Some(Label::new("Caractère interdit", "Invalid character"));
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Some(Label::new("Point ou espace final", "Trailing dot or space"));
    }
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return Some(Label::new("Nom réservé par Windows", "Name reserved by Windows"));
    }
    if name.len() > 255 {
        return Some(Label::new("Nom trop long", "Name too long"));
    }
    None
}

/// Si `target` existe, ajoute ` (1)`, ` (2)`... avant l'extension.
pub fn unique_path(target: &Path) -> PathBuf {
    if !target.exists() {
        return target.to_path_buf();
    }
    let parent = target.parent().unwrap_or(Path::new(""));
    let stem = target.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = target.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (1u32..)
        .map(|i| parent.join(format!("{stem} ({i}){ext}")))
        .find(|p| !p.exists())
        .expect("compteur epuise")
}

/// Cle de comparaison de chemin : minuscules, separateurs uniformes, sans `\` final.
pub fn path_key(path: &Path) -> String {
    let mut s = path.to_string_lossy().replace('/', "\\").to_lowercase();
    while s.len() > 3 && s.ends_with('\\') {
        s.pop();
    }
    s
}

/// Vrai si `path` est `parent` ou se trouve sous `parent`.
pub fn is_within(path: &Path, parent: &Path) -> bool {
    let (p, q) = (path_key(path), path_key(parent));
    p == q || (p.starts_with(&q) && (q.ends_with('\\') || p.as_bytes().get(q.len()) == Some(&b'\\')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_localized() {
        assert_eq!(format_size(512, Lang::Fr), "512 o");
        assert_eq!(format_size(1536, Lang::Fr), "1,50 Ko");
        assert_eq!(format_size(1536, Lang::En), "1.50 KB");
        assert_eq!(format_size(150 * 1024 * 1024, Lang::En), "150 MB");
    }

    #[test]
    fn counts_have_separators() {
        assert_eq!(format_count(1234567, Lang::En), "1,234,567");
        assert_eq!(format_count(999, Lang::En), "999");
    }

    #[test]
    fn accents_are_removed() {
        assert_eq!(strip_accents("Été à Noël"), "Ete a Noel");
    }

    #[test]
    fn invalid_names_are_rejected() {
        assert!(filename_problem("a:b.txt").is_some());
        assert!(filename_problem("CON.txt").is_some());
        assert!(filename_problem("fin.").is_some());
        assert!(filename_problem("ok name.txt").is_none());
    }

    #[test]
    fn within_respects_component_boundaries() {
        assert!(is_within(Path::new(r"C:\a\b"), Path::new(r"c:\A")));
        assert!(is_within(Path::new(r"C:\a"), Path::new(r"C:\a\")));
        assert!(!is_within(Path::new(r"C:\ab"), Path::new(r"C:\a")));
        assert!(is_within(Path::new(r"C:\a"), Path::new(r"C:\")));
    }
}
