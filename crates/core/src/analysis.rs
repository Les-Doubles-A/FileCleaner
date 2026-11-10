//! Analyses instantanees derivees d'un scan : fichiers inutiles, gros fichiers,
//! anciens, dossiers vides, extensions rares, artefacts de developpement.

use std::collections::{HashMap, HashSet};

use glob::{MatchOptions, Pattern};

use crate::config::Settings;
use crate::model::{DirId, FileId, NO_DIR, Scan};
use crate::util::{Label, now_ts};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Risk {
    /// Regenerable ou sans valeur : proposable en selection rapide.
    Safe,
    /// A verifier : peut contenir quelque chose d'utile.
    Review,
}

pub struct JunkRule {
    pub label: Label,
    pub risk: Risk,
    pub patterns: &'static [&'static str],
}

pub const JUNK_RULES: &[JunkRule] = &[
    JunkRule {
        label: Label::new("Fichiers temporaires", "Temporary files"),
        risk: Risk::Safe,
        patterns: &["*.tmp", "*.temp", "~$*", "*.~*", "*.$$$"],
    },
    JunkRule {
        label: Label::new("Rapports de plantage", "Crash dumps"),
        risk: Risk::Safe,
        patterns: &["*.dmp", "*.mdmp", "*.stackdump", "hs_err_pid*.log"],
    },
    JunkRule {
        label: Label::new("Miniatures et index système", "Thumbnails and system indexes"),
        risk: Risk::Safe,
        patterns: &["thumbs.db", "ehthumbs.db", ".ds_store", "._*"],
    },
    JunkRule {
        label: Label::new("Bytecode compilé", "Compiled bytecode"),
        risk: Risk::Safe,
        patterns: &["*.pyc", "*.pyo"],
    },
    JunkRule {
        label: Label::new("Journaux", "Log files"),
        risk: Risk::Review,
        patterns: &["*.log", "*.log.?", "*.log.old"],
    },
    JunkRule {
        label: Label::new("Sauvegardes automatiques", "Automatic backups"),
        risk: Risk::Review,
        patterns: &["*.bak", "*.old", "*.orig", "*.backup", "*~", "*.swp", "*.swo"],
    },
    JunkRule {
        label: Label::new("Téléchargements interrompus", "Interrupted downloads"),
        risk: Risk::Review,
        patterns: &["*.crdownload", "*.part", "*.partial", "*.download"],
    },
    JunkRule {
        label: Label::new("Configuration d'affichage", "Folder view settings"),
        risk: Risk::Review,
        patterns: &["desktop.ini"],
    },
];

pub const RULE_CACHE_DIR: usize = JUNK_RULES.len();
pub const RULE_EMPTY_FILE: usize = JUNK_RULES.len() + 1;
pub const RULE_EXTRACTED: usize = JUNK_RULES.len() + 2;
pub const RULE_CUSTOM: usize = JUNK_RULES.len() + 3;
pub const RULE_COUNT: usize = JUNK_RULES.len() + 4;

pub fn rule_label(rule: usize) -> Label {
    match rule {
        RULE_CACHE_DIR => Label::new("Contenu de dossiers cache", "Cache folder content"),
        RULE_EMPTY_FILE => Label::new("Fichiers vides (0 octet)", "Empty files (0 bytes)"),
        RULE_EXTRACTED => Label::new("Archives déjà extraites", "Already extracted archives"),
        RULE_CUSTOM => Label::new("Règles personnalisées", "Custom rules"),
        other => JUNK_RULES[other].label,
    }
}

pub fn rule_risk(rule: usize) -> Risk {
    JUNK_RULES.get(rule).map_or(Risk::Review, |r| r.risk)
}

#[derive(Clone, Copy, Debug)]
pub struct JunkHit {
    pub file: FileId,
    pub rule: usize,
}

const CACHE_DIR_NAMES: &[&str] = &["cache", ".cache", "thumbnails", ".thumbnails", "__pycache__"];
const ARCHIVE_EXTS: &[&str] = &["zip", "rar", "7z", "tar", "tgz"];

/// Certains fichiers vides ont un role : marqueurs de paquets ou manifestes de projet.
fn is_meaningful_empty(name: &str, scan: &Scan, dir: DirId) -> bool {
    const KEEP: &[&str] = &["__init__.py", ".gitkeep", ".keep", ".nojekyll", "py.typed", "cargo.toml", "package.json"];
    let lower = name.to_lowercase();
    KEEP.contains(&lower.as_str()) || lower.starts_with('.') || is_inside_project(scan, dir)
}

pub fn find_junk(scan: &Scan, settings: &Settings) -> Vec<JunkHit> {
    let options = MatchOptions { case_sensitive: false, ..MatchOptions::default() };
    let compiled: Vec<(usize, Pattern)> = JUNK_RULES
        .iter()
        .enumerate()
        .flat_map(|(i, rule)| {
            rule.patterns.iter().filter_map(move |p| Pattern::new(p).ok().map(|p| (i, p)))
        })
        .chain(settings.custom_junk_patterns.iter().filter_map(|p| {
            Pattern::new(p.trim()).ok().map(|p| (RULE_CUSTOM, p))
        }))
        .collect();

    let mut hits = Vec::new();
    for (id, f) in scan.alive() {
        let rule = compiled.iter().find(|(_, p)| p.matches_with(&f.name, options)).map(|(i, _)| *i);
        let rule = rule.or_else(|| {
            let dir = scan.dir(f.dir);
            let lower = dir.name.to_lowercase();
            if CACHE_DIR_NAMES.contains(&lower.as_str()) {
                return Some(RULE_CACHE_DIR);
            }
            if f.size == 0 && !is_meaningful_empty(&f.name, scan, f.dir) {
                return Some(RULE_EMPTY_FILE);
            }
            if ARCHIVE_EXTS.contains(&scan.ext_of(f)) {
                let stem = f.name.rsplit_once('.').map_or(&*f.name, |(s, _)| s);
                let extracted = dir.children.iter().any(|&c| {
                    let child = scan.dir(c);
                    child.name.eq_ignore_ascii_case(stem) && child.file_count > 0
                });
                if extracted {
                    return Some(RULE_EXTRACTED);
                }
            }
            None
        });
        if let Some(rule) = rule {
            hits.push(JunkHit { file: id, rule });
        }
    }
    hits
}

pub fn find_big(scan: &Scan, min_bytes: u64) -> Vec<FileId> {
    let mut out: Vec<FileId> = scan.alive().filter(|(_, f)| f.size >= min_bytes).map(|(id, _)| id).collect();
    out.sort_by_key(|&id| std::cmp::Reverse(scan.file(id).size));
    out
}

pub fn find_old(scan: &Scan, min_days: u32) -> Vec<FileId> {
    let limit = now_ts() - i64::from(min_days) * 86_400;
    let mut out: Vec<FileId> =
        scan.alive().filter(|(_, f)| f.modified > 0 && f.modified <= limit).map(|(id, _)| id).collect();
    out.sort_by_key(|&id| scan.file(id).modified);
    out
}

/// Dossiers reellement vides, sous-dossiers inclus. Seul le dossier le plus haut
/// d'une branche vide est retourne.
pub fn find_empty_dirs(scan: &Scan) -> Vec<DirId> {
    fn subtree_clean(scan: &Scan, id: DirId) -> bool {
        let node = scan.dir(id);
        !node.hidden_entries
            && node.file_count == 0
            && node.children.iter().all(|&c| subtree_clean(scan, c))
    }
    fn visit(scan: &Scan, id: DirId, out: &mut Vec<DirId>) {
        for &child in &scan.dir(id).children {
            if subtree_clean(scan, child) {
                out.push(child);
            } else {
                visit(scan, child, out);
            }
        }
    }
    let mut out = Vec::new();
    for &root in &scan.root_dirs {
        visit(scan, root, &mut out);
    }
    out
}

pub fn find_rare_exts(scan: &Scan, max_occurrences: u32) -> Vec<FileId> {
    let mut counts: HashMap<u32, u32> = HashMap::new();
    for (_, f) in scan.alive() {
        *counts.entry(f.ext).or_default() += 1;
    }
    let rare: HashSet<u32> =
        counts.into_iter().filter(|(_, n)| *n <= max_occurrences).map(|(e, _)| e).collect();
    let mut out: Vec<FileId> =
        scan.alive().filter(|(_, f)| rare.contains(&f.ext)).map(|(id, _)| id).collect();
    out.sort_by(|&a, &b| {
        let (fa, fb) = (scan.file(a), scan.file(b));
        scan.ext_of(fa).cmp(scan.ext_of(fb)).then(fb.size.cmp(&fa.size))
    });
    out
}

struct DevRule {
    dir: &'static str,
    /// Fichier ou extension (`*.csproj`) qui doit exister a cote du dossier. Vide = aucun.
    markers: &'static [&'static str],
    kind: &'static str,
}

const DEV_RULES: &[DevRule] = &[
    DevRule { dir: "node_modules", markers: &["package.json"], kind: "Node.js" },
    DevRule { dir: "target", markers: &["cargo.toml"], kind: "Rust" },
    DevRule { dir: "__pycache__", markers: &[], kind: "Python" },
    DevRule { dir: ".venv", markers: &[], kind: "Python" },
    DevRule { dir: "venv", markers: &["requirements.txt", "pyproject.toml", "setup.py"], kind: "Python" },
    DevRule { dir: ".pytest_cache", markers: &[], kind: "Python" },
    DevRule { dir: ".mypy_cache", markers: &[], kind: "Python" },
    DevRule { dir: ".ruff_cache", markers: &[], kind: "Python" },
    DevRule { dir: ".tox", markers: &[], kind: "Python" },
    DevRule { dir: ".next", markers: &["package.json"], kind: "Next.js" },
    DevRule { dir: ".nuxt", markers: &["package.json"], kind: "Nuxt" },
    DevRule { dir: ".angular", markers: &["angular.json"], kind: "Angular" },
    DevRule { dir: ".turbo", markers: &[], kind: "Turborepo" },
    DevRule { dir: ".parcel-cache", markers: &[], kind: "Parcel" },
    DevRule { dir: "dist", markers: &["package.json"], kind: "JavaScript" },
    DevRule { dir: "build", markers: &["package.json", "build.gradle", "build.gradle.kts", "cmakelists.txt"], kind: "Build" },
    DevRule { dir: ".gradle", markers: &[], kind: "Gradle" },
    DevRule { dir: "bin", markers: &["*.csproj", "*.fsproj", "*.vbproj"], kind: ".NET" },
    DevRule { dir: "obj", markers: &["*.csproj", "*.fsproj", "*.vbproj"], kind: ".NET" },
    DevRule { dir: "pods", markers: &["podfile"], kind: "CocoaPods" },
    DevRule { dir: ".terraform", markers: &[], kind: "Terraform" },
    DevRule { dir: "deriveddata", markers: &[], kind: "Xcode" },
];

#[derive(Clone, Debug)]
pub struct DevArtifact {
    pub dir: DirId,
    pub kind: &'static str,
    pub size: u64,
    pub files: u32,
    /// Dossier du projet proprietaire.
    pub project: String,
    pub last_modified: i64,
}

fn has_marker(scan: &Scan, parent: DirId, markers: &[&str]) -> bool {
    if markers.is_empty() {
        return true;
    }
    let node = scan.dir(parent);
    node.files.clone().any(|id| {
        let file = scan.file(id);
        let name = file.name.to_lowercase();
        markers.iter().any(|m| match m.strip_prefix('*') {
            Some(suffix) => name.ends_with(suffix),
            None => name == *m,
        })
    })
}

/// Dossiers regenerables de projets. Les artefacts imbriques dans un autre ne
/// sont pas listes.
pub fn find_dev_artifacts(scan: &Scan) -> Vec<DevArtifact> {
    fn visit(scan: &Scan, id: DirId, out: &mut Vec<DevArtifact>) {
        let node = scan.dir(id);
        for &child in &node.children {
            let c = scan.dir(child);
            let lower = c.name.to_lowercase();
            let rule = DEV_RULES.iter().find(|r| r.dir == lower && has_marker(scan, id, r.markers));
            match rule {
                Some(rule) if c.size > 0 => out.push(DevArtifact {
                    dir: child,
                    kind: rule.kind,
                    size: c.size,
                    files: c.file_count,
                    project: node.name.to_string(),
                    last_modified: c.modified,
                }),
                _ => visit(scan, child, out),
            }
        }
    }
    let mut out = Vec::new();
    for &root in &scan.root_dirs {
        visit(scan, root, &mut out);
    }
    out.sort_by_key(|a| std::cmp::Reverse(a.size));
    out
}

/// Dossier sous un projet detecte (protection du classement automatique).
pub fn is_inside_project(scan: &Scan, mut dir: DirId) -> bool {
    const MARKERS: &[&str] = &[
        "package.json", "cargo.toml", "pyproject.toml", "requirements.txt", "pom.xml",
        "build.gradle", "cmakelists.txt", "makefile", "go.mod", "composer.json", "gemfile",
    ];
    while dir != NO_DIR {
        let node = scan.dir(dir);
        if node.path.join(".git").exists() {
            return true;
        }
        let found = node.files.clone().any(|id| {
            let name = scan.file(id).name.to_lowercase();
            MARKERS.contains(&name.as_str()) || name.ends_with(".sln") || name.ends_with(".csproj")
        });
        if found {
            return true;
        }
        dir = node.parent;
    }
    false
}

/// Synthese affichee sur le tableau de bord.
#[derive(Clone, Debug, Default)]
pub struct Insights {
    pub junk_files: u64,
    pub junk_bytes: u64,
    pub junk_safe_bytes: u64,
    pub big_files: u64,
    pub big_bytes: u64,
    pub old_files: u64,
    pub old_bytes: u64,
    pub empty_dirs: u64,
    pub dev_dirs: u64,
    pub dev_bytes: u64,
}

impl Insights {
    pub fn compute(scan: &Scan, settings: &Settings) -> Self {
        let junk = find_junk(scan, settings);
        let dev = find_dev_artifacts(scan);
        let empty = find_empty_dirs(scan).len();
        Self::from_parts(scan, settings, &junk, &dev, empty)
    }

    /// Synthese a partir d'analyses deja calculees.
    pub fn from_parts(scan: &Scan, settings: &Settings, junk: &[JunkHit], dev: &[DevArtifact], empty: usize) -> Self {
        let big = find_big(scan, settings.big_file_threshold_mb * 1024 * 1024);
        let old = find_old(scan, settings.old_file_threshold_days);
        let bytes = |ids: &[FileId]| ids.iter().map(|&id| scan.file(id).size).sum::<u64>();
        Self {
            junk_files: junk.len() as u64,
            junk_bytes: junk.iter().map(|h| scan.file(h.file).size).sum(),
            junk_safe_bytes: junk
                .iter()
                .filter(|h| rule_risk(h.rule) == Risk::Safe)
                .map(|h| scan.file(h.file).size)
                .sum(),
            big_files: big.len() as u64,
            big_bytes: bytes(&big),
            old_files: old.len() as u64,
            old_bytes: bytes(&old),
            empty_dirs: empty as u64,
            dev_dirs: dev.len() as u64,
            dev_bytes: dev.iter().map(|d| d.size).sum(),
        }
    }

    /// Score de 0 a 100 : part de l'espace occupe par du recuperable.
    pub fn health_score(&self, total: u64, duplicate_bytes: u64) -> u8 {
        if total == 0 {
            return 100;
        }
        let wasted = self.junk_bytes + self.dev_bytes + duplicate_bytes;
        let ratio = (wasted as f64 / total as f64).min(1.0);
        let clutter = (self.empty_dirs as f64).min(200.0) / 200.0 * 5.0;
        // Courbe racine : 1 % de perte coute deja 10 points, 25 % en coute 50.
        (100.0 - ratio.sqrt() * 100.0 - clutter).clamp(0.0, 100.0) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Progress;
    use crate::scanner::scan;
    use std::fs;
    use std::path::Path;

    fn scan_of(dir: &Path) -> Scan {
        scan(&[dir.to_path_buf()], &Settings::default(), &Progress::default())
    }

    fn write(root: &Path, rel: &str, bytes: &[u8]) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn junk_rules_and_risks() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        write(r, "a.TMP", b"x");
        write(r, "app.log", b"x");
        write(r, "Thumbs.db", b"x");
        write(r, "keep.docx", b"x");
        write(r, "cache/blob", b"x");
        write(r, "zero.txt", b"");
        write(r, "lib/__init__.py", b"");
        write(r, "pack.zip", b"zip");
        write(r, "pack/inner.txt", b"x");
        let s = scan_of(r);
        let hits = find_junk(&s, &Settings::default());
        let rule_of = |name: &str| {
            hits.iter().find(|h| &*s.file(h.file).name == name).map(|h| h.rule)
        };
        assert_eq!(rule_risk(rule_of("a.TMP").unwrap()), Risk::Safe);
        assert_eq!(rule_risk(rule_of("app.log").unwrap()), Risk::Review);
        assert_eq!(rule_risk(rule_of("Thumbs.db").unwrap()), Risk::Safe);
        assert_eq!(rule_of("blob"), Some(RULE_CACHE_DIR));
        assert_eq!(rule_of("zero.txt"), Some(RULE_EMPTY_FILE));
        assert_eq!(rule_of("pack.zip"), Some(RULE_EXTRACTED));
        assert_eq!(rule_of("keep.docx"), None);
        assert_eq!(rule_of("__init__.py"), None);
    }

    #[test]
    fn custom_junk_patterns_apply() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "x.foo", b"1");
        let s = scan_of(dir.path());
        let settings = Settings { custom_junk_patterns: vec!["*.foo".into()], ..Settings::default() };
        assert_eq!(find_junk(&s, &settings)[0].rule, RULE_CUSTOM);
    }

    #[test]
    fn empty_detection_is_deep_and_topmost() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        fs::create_dir_all(r.join("e1/e2/e3")).unwrap();
        fs::create_dir_all(r.join("full/e4")).unwrap();
        write(r, "full/file.txt", b"x");
        let s = scan_of(r);
        let mut names: Vec<_> = find_empty_dirs(&s).iter().map(|&d| s.dir(d).name.to_string()).collect();
        names.sort();
        assert_eq!(names, ["e1", "e4"]);
    }

    #[test]
    fn root_is_never_reported_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_empty_dirs(&scan_of(dir.path())).is_empty());
    }

    #[test]
    fn folder_with_filtered_content_is_not_empty() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "sub/secret.pdf", b"x");
        let settings = Settings { ignored_extensions: vec!["pdf".into()], ..Settings::default() };
        let s = scan(&[dir.path().to_path_buf()], &settings, &Progress::default());
        assert!(find_empty_dirs(&s).is_empty());
    }

    #[test]
    fn dev_artifacts_need_their_marker() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        write(r, "proj/package.json", b"{}");
        write(r, "proj/node_modules/a/index.js", b"x");
        write(r, "proj/node_modules/a/node_modules/b/i.js", b"x");
        write(r, "rust/Cargo.toml", b"");
        write(r, "rust/target/debug/app.exe", b"xx");
        write(r, "docs/target/report.pdf", b"keep me");
        write(r, "net/app.csproj", b"");
        write(r, "net/bin/app.dll", b"x");
        let s = scan_of(r);
        let found = find_dev_artifacts(&s);
        let mut kinds: Vec<_> = found.iter().map(|a| a.kind).collect();
        kinds.sort();
        assert_eq!(kinds, [".NET", "Node.js", "Rust"]);
    }

    #[test]
    fn rare_extensions() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..5 {
            write(dir.path(), &format!("c{i}.txt"), b"x");
        }
        write(dir.path(), "odd.xyz", b"x");
        let s = scan_of(dir.path());
        let rare = find_rare_exts(&s, 3);
        assert_eq!(rare.len(), 1);
        assert_eq!(&*s.file(rare[0]).name, "odd.xyz");
    }

    #[test]
    fn score_decreases_with_waste() {
        let clean = Insights::default();
        let dirty = Insights { junk_bytes: 25, ..Insights::default() };
        assert_eq!(clean.health_score(100, 0), 100);
        assert_eq!(dirty.health_score(100, 0), 50);
        assert_eq!(clean.health_score(0, 0), 100);
    }
}
