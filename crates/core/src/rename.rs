//! Renommage par lots : gabarit, rechercher/remplacer, casse, nettoyage.
//! Le plan est toujours calcule et valide avant toute ecriture.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;

use anyhow::Result;
use regex::{NoExpand, Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::history::{OpKind, Operation, Session};
use crate::model::{FileId, Scan};
use crate::util::{Label, filename_problem, sanitize_component, strip_accents, year_month_day};

static DATE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[^0-9])((?:19|20)\d{2})[-_.]?(0[1-9]|1[0-2])[-_.]?(0[1-9]|[12]\d|3[01])(?:[^0-9]|$)")
        .expect("regex de date")
});
static CAMERA_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(IMG|IMAGE|DSC|DCIM|PHOTO|VID|VIDEO|SCREENSHOT|SCR|PXL|WA)[_\- ]?")
        .expect("regex prefixe")
});
static COUNTER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*(\(\d+\)|- ?(Copie|Copy)( \(\d+\))?)$").expect("regex compteur"));
static LONG_DIGITS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b\d{6,}\b").expect("regex"));
static SEPARATORS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ _\-]{2,}").expect("regex"));

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CaseMode {
    #[default]
    Keep,
    Lower,
    Upper,
    Title,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SpaceMode {
    #[default]
    Keep,
    Underscore,
    Dash,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenameOptions {
    pub template: String,
    pub find: String,
    pub replace: String,
    pub use_regex: bool,
    pub case: CaseMode,
    pub spaces: SpaceMode,
    pub strip_accents: bool,
    pub clean_noise: bool,
    pub lowercase_ext: bool,
    pub start: u32,
    pub pad: usize,
    pub counter_per_folder: bool,
}

impl Default for RenameOptions {
    fn default() -> Self {
        Self {
            template: "{name}".into(),
            find: String::new(),
            replace: String::new(),
            use_regex: false,
            case: CaseMode::Keep,
            spaces: SpaceMode::Keep,
            strip_accents: false,
            clean_noise: false,
            lowercase_ext: false,
            start: 1,
            pad: 3,
            counter_per_folder: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenameStatus {
    Ready,
    Unchanged,
    /// Suffixe ajoute automatiquement pour eviter un conflit.
    Adjusted,
    Invalid(Label),
}

#[derive(Clone, Debug)]
pub struct RenameItem {
    pub file: FileId,
    pub old_name: String,
    pub new_name: String,
    pub status: RenameStatus,
}

impl RenameItem {
    pub fn will_apply(&self) -> bool {
        matches!(self.status, RenameStatus::Ready | RenameStatus::Adjusted)
    }
}

/// Date `AAAA-MM-JJ` trouvee dans un nom (IMG_20260304, 2026-03-04...).
pub fn date_in_name(name: &str) -> Option<String> {
    DATE_RE.captures(name).map(|c| format!("{}-{}-{}", &c[1], &c[2], &c[3]))
}

/// Retire prefixes d'appareil, compteurs de copie et suites de chiffres.
pub fn clean_noise(stem: &str) -> String {
    let cleaned = CAMERA_RE.replace(stem, "");
    let cleaned = COUNTER_RE.replace(&cleaned, "");
    let cleaned = LONG_DIGITS_RE.replace_all(&cleaned, "");
    let cleaned = SEPARATORS_RE.replace_all(&cleaned, " ");
    let cleaned = cleaned.trim_matches(|c: char| c == ' ' || c == '-' || c == '_');
    if cleaned.is_empty() { stem.to_string() } else { cleaned.to_string() }
}

fn title_case(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut start = true;
    for c in text.chars() {
        if start {
            out.extend(c.to_uppercase());
        } else {
            out.extend(c.to_lowercase());
        }
        start = !c.is_alphanumeric();
    }
    out
}

fn split_name(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(0) | None => (name, ""),
        Some(i) => (&name[..i], &name[i + 1..]),
    }
}

/// Compile le motif de recherche ; `Err` si l'expression reguliere est invalide.
fn compile_find(options: &RenameOptions) -> Result<Option<Regex>, String> {
    if options.find.is_empty() {
        return Ok(None);
    }
    let pattern = if options.use_regex { options.find.clone() } else { regex::escape(&options.find) };
    RegexBuilder::new(&pattern)
        .case_insensitive(!options.use_regex)
        .build()
        .map(Some)
        .map_err(|err| err.to_string())
}

fn apply_transforms(stem: &str, options: &RenameOptions, find: Option<&Regex>) -> String {
    let mut text = stem.to_string();
    if options.clean_noise {
        text = clean_noise(&text);
    }
    if let Some(re) = find {
        text = if options.use_regex {
            re.replace_all(&text, options.replace.as_str()).into_owned()
        } else {
            re.replace_all(&text, NoExpand(&options.replace)).into_owned()
        };
    }
    text
}

fn finish(text: String, options: &RenameOptions) -> String {
    let mut text = text;
    if options.strip_accents {
        text = strip_accents(&text);
    }
    text = match options.case {
        CaseMode::Keep => text,
        CaseMode::Lower => text.to_lowercase(),
        CaseMode::Upper => text.to_uppercase(),
        CaseMode::Title => title_case(&text),
    };
    match options.spaces {
        SpaceMode::Keep => text,
        SpaceMode::Underscore => text.replace(' ', "_"),
        SpaceMode::Dash => text.replace(' ', "-"),
    }
}

/// Construit le nouveau nom d'un fichier.
fn build_name(scan: &Scan, id: FileId, counter: u32, options: &RenameOptions, find: Option<&Regex>) -> String {
    let file = scan.file(id);
    let (stem, ext) = split_name(&file.name);
    let name = apply_transforms(stem, options, find);
    let (year, month, day) = year_month_day(file.modified);
    let mdate = format!("{year:04}-{month:02}-{day:02}");
    let date = date_in_name(&file.name).unwrap_or_else(|| mdate.clone());
    let parent = scan.dir(file.dir).name.to_string();
    let number = format!("{counter:0width$}", width = options.pad);

    let tokens: [(&str, &str); 8] = [
        ("{name}", &name),
        ("{n}", &number),
        ("{date}", &date),
        ("{mdate}", &mdate),
        ("{yyyy}", &format!("{year:04}")),
        ("{mm}", &format!("{month:02}")),
        ("{dd}", &format!("{day:02}")),
        ("{parent}", &parent),
    ];
    let mut text = options.template.clone();
    for (token, value) in tokens {
        text = text.replace(token, value);
    }
    let text = finish(text, options);
    let ext = if options.lowercase_ext { ext.to_lowercase() } else { ext.to_string() };
    if ext.is_empty() { text } else { format!("{text}.{ext}") }
}

/// Calcule le plan. `Err` uniquement si la regex de recherche est invalide.
pub fn build_plan(scan: &Scan, files: &[FileId], options: &RenameOptions) -> Result<Vec<RenameItem>, String> {
    let find = compile_find(options)?;
    let mut counters: HashMap<u32, u32> = HashMap::new();
    let mut global = options.start;
    // Noms occupes par dossier : les fichiers hors plan restent intouchables.
    let in_plan: HashSet<FileId> = files.iter().copied().collect();
    let mut taken: HashMap<u32, HashSet<String>> = HashMap::new();
    let mut plan = Vec::with_capacity(files.len());

    for &id in files {
        let file = scan.file(id);
        let used = taken.entry(file.dir).or_insert_with(|| {
            scan.dir(file.dir)
                .files
                .clone()
                .filter(|&other| !in_plan.contains(&other) && scan.file(other).is_alive())
                .map(|other| scan.file(other).name.to_lowercase())
                .collect()
        });
        let counter = if options.counter_per_folder {
            let c = counters.entry(file.dir).or_insert(options.start);
            let value = *c;
            *c += 1;
            value
        } else {
            let value = global;
            global += 1;
            value
        };

        let wanted = build_name(scan, id, counter, options, find.as_ref());
        let old_name = file.name.to_string();
        if let Some(problem) = filename_problem(&wanted) {
            used.insert(old_name.to_lowercase());
            plan.push(RenameItem { file: id, old_name, new_name: wanted, status: RenameStatus::Invalid(problem) });
            continue;
        }
        let (mut new_name, mut status) = (wanted.clone(), RenameStatus::Ready);
        if used.contains(&new_name.to_lowercase()) {
            let (stem, ext) = split_name(&wanted);
            let dot = if ext.is_empty() { "" } else { "." };
            new_name = (2u32..)
                .map(|i| format!("{stem}_{i}{dot}{ext}"))
                .find(|candidate| !used.contains(&candidate.to_lowercase()))
                .expect("compteur epuise");
            status = RenameStatus::Adjusted;
        }
        if new_name == old_name {
            status = RenameStatus::Unchanged;
        }
        used.insert(new_name.to_lowercase());
        plan.push(RenameItem { file: id, old_name, new_name, status });
    }
    Ok(plan)
}

#[derive(Debug, Default)]
pub struct RenameReport {
    pub session: Option<Session>,
    pub renamed: HashMap<FileId, String>,
    pub failed: Vec<(PathBuf, String)>,
}

/// Applique le plan en deux temps (noms temporaires) pour autoriser
/// les permutations du type `a -> b`, `b -> a`.
pub fn apply_plan(scan: &Scan, plan: &[RenameItem], label: &str) -> RenameReport {
    let mut report = RenameReport::default();
    let mut session = Session::new(label, &scan.title());
    let items: Vec<&RenameItem> = plan.iter().filter(|i| i.will_apply()).collect();
    let sources: HashSet<(u32, String)> =
        items.iter().map(|i| (scan.file(i.file).dir, i.old_name.to_lowercase())).collect();

    // Etape 1 : les fichiers dont le nom cible est celui d'une source passent par un nom temporaire.
    let mut staged: Vec<(&RenameItem, PathBuf, PathBuf)> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let folder = scan.folder_of(item.file);
        let source = folder.join(&item.old_name);
        let target = folder.join(&item.new_name);
        let dir = scan.file(item.file).dir;
        let same_file = item.old_name.eq_ignore_ascii_case(&item.new_name);
        let blocked_by_source = sources.contains(&(dir, item.new_name.to_lowercase())) && !same_file;
        if blocked_by_source {
            let temp = folder.join(format!(".fc-rename-{}-{index}.tmp", std::process::id()));
            if let Err(err) = fs::rename(&source, &temp) {
                report.failed.push((source, err.to_string()));
                continue;
            }
            staged.push((item, temp, target));
        } else {
            staged.push((item, source, target));
        }
    }

    // Etape 2 : pose des noms definitifs, sans jamais ecraser.
    for (item, current, target) in staged {
        let original = scan.folder_of(item.file).join(&item.old_name);
        let same_file = item.old_name.eq_ignore_ascii_case(&item.new_name);
        let outcome = if target.exists() && !same_file {
            Err("Un fichier porte déjà ce nom".to_string())
        } else {
            fs::rename(&current, &target).map_err(|err| err.to_string())
        };
        match outcome {
            Ok(()) => {
                session.operations.push(Operation {
                    kind: OpKind::Rename,
                    source: original,
                    destination: Some(target),
                    size: scan.file(item.file).size,
                    is_dir: false,
                    restored: false,
                    purged: false,
                });
                report.renamed.insert(item.file, item.new_name.clone());
            }
            Err(message) => {
                if current != original {
                    let _ = fs::rename(&current, &original);
                }
                report.failed.push((original, message));
            }
        }
    }
    report.session = Some(session);
    report
}

/// Nettoie un segment de chemin produit par un gabarit.
pub fn safe_segment(text: &str) -> String {
    let cleaned = sanitize_component(text);
    if cleaned.is_empty() { "_".into() } else { cleaned }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::model::Progress;
    use crate::scanner::scan;

    fn fixture(names: &[&str]) -> (tempfile::TempDir, Scan, Vec<FileId>) {
        let dir = tempfile::tempdir().unwrap();
        for name in names {
            fs::write(dir.path().join(name), name.as_bytes()).unwrap();
        }
        let s = scan(&[dir.path().to_path_buf()], &Settings::default(), &Progress::default());
        let mut ids: Vec<FileId> = s.alive().map(|(id, _)| id).collect();
        ids.sort_by_key(|&id| s.file(id).name.clone());
        (dir, s, ids)
    }

    fn new_names(plan: &[RenameItem]) -> Vec<String> {
        plan.iter().map(|i| i.new_name.clone()).collect()
    }

    #[test]
    fn dates_are_found_in_common_shapes() {
        assert_eq!(date_in_name("IMG_20260304_145021.jpg").unwrap(), "2026-03-04");
        assert_eq!(date_in_name("rapport 2024-12-31.pdf").unwrap(), "2024-12-31");
        assert_eq!(date_in_name("v12345678"), None);
        assert_eq!(date_in_name("20261399.txt"), None);
    }

    #[test]
    fn noise_is_cleaned() {
        assert_eq!(clean_noise("IMG_Vacances (2)"), "Vacances");
        assert_eq!(clean_noise("Rapport - Copie"), "Rapport");
        assert_eq!(clean_noise("IMG_20260304"), "IMG_20260304");
    }

    #[test]
    fn template_counters_and_extension() {
        let (_d, s, ids) = fixture(&["b.JPG", "a.png"]);
        let options = RenameOptions { template: "Paris_{n}".into(), lowercase_ext: true, ..Default::default() };
        let plan = build_plan(&s, &ids, &options).unwrap();
        assert_eq!(new_names(&plan), ["Paris_001.png", "Paris_002.jpg"]);
    }

    #[test]
    fn find_replace_literal_and_regex() {
        let (_d, s, ids) = fixture(&["foo (1).txt"]);
        let options = RenameOptions { find: "(1)".into(), replace: "v$1".into(), ..Default::default() };
        assert_eq!(new_names(&build_plan(&s, &ids, &options).unwrap()), ["foo v$1.txt"]);
        let options = RenameOptions {
            find: r"\((\d)\)".into(),
            replace: "v$1".into(),
            use_regex: true,
            ..Default::default()
        };
        assert_eq!(new_names(&build_plan(&s, &ids, &options).unwrap()), ["foo v1.txt"]);
        let options = RenameOptions { find: "(".into(), use_regex: true, ..Default::default() };
        assert!(build_plan(&s, &ids, &options).is_err());
    }

    #[test]
    fn collisions_with_outside_files_get_suffixed() {
        let (_d, s, ids) = fixture(&["a.txt", "b.txt"]);
        let options = RenameOptions { template: "b".into(), ..Default::default() };
        let plan = build_plan(&s, &ids[..1], &options).unwrap();
        assert_eq!(plan[0].new_name, "b_2.txt");
        assert_eq!(plan[0].status, RenameStatus::Adjusted);
    }

    #[test]
    fn collisions_inside_plan_get_suffixed() {
        let (_d, s, ids) = fixture(&["a.txt", "b.txt"]);
        let options = RenameOptions { template: "same".into(), ..Default::default() };
        assert_eq!(new_names(&build_plan(&s, &ids, &options).unwrap()), ["same.txt", "same_2.txt"]);
    }

    #[test]
    fn invalid_names_are_blocked() {
        let (_d, s, ids) = fixture(&["a.txt"]);
        let options = RenameOptions { template: "bad:name".into(), ..Default::default() };
        let plan = build_plan(&s, &ids, &options).unwrap();
        assert!(!plan[0].will_apply());
    }

    #[test]
    fn apply_handles_swaps_and_case_changes() {
        let (dir, s, ids) = fixture(&["a.txt", "b.txt", "Case.txt"]);
        let by_name = |n: &str| ids.iter().copied().find(|&id| &*s.file(id).name == n).unwrap();
        let item = |id, old: &str, new: &str| RenameItem {
            file: id,
            old_name: old.into(),
            new_name: new.into(),
            status: RenameStatus::Ready,
        };
        let plan = vec![
            item(by_name("a.txt"), "a.txt", "b.txt"),
            item(by_name("b.txt"), "b.txt", "a.txt"),
            item(by_name("Case.txt"), "Case.txt", "case.txt"),
        ];
        let report = apply_plan(&s, &plan, "t");
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert_eq!(fs::read(dir.path().join("b.txt")).unwrap(), b"a.txt");
        assert_eq!(fs::read(dir.path().join("a.txt")).unwrap(), b"b.txt");
        let listed: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(listed.contains(&"case.txt".to_string()));
        assert_eq!(listed.len(), 3, "aucun temporaire ne doit rester");
    }

    #[test]
    fn apply_refuses_to_overwrite_late_arrival() {
        let (dir, s, ids) = fixture(&["a.txt"]);
        let options = RenameOptions { template: "z".into(), ..Default::default() };
        let plan = build_plan(&s, &ids, &options).unwrap();
        fs::write(dir.path().join("z.txt"), b"arrived").unwrap();
        let report = apply_plan(&s, &plan, "t");
        assert_eq!(report.failed.len(), 1);
        assert_eq!(fs::read(dir.path().join("z.txt")).unwrap(), b"arrived");
        assert!(dir.path().join("a.txt").exists());
    }
}
