//! Classement automatique par gabarit de chemin.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crate::analysis::is_inside_project;
use crate::config::category_label;
use crate::model::{DirId, FileId, Scan};
use crate::rename::safe_segment;
use crate::util::{Lang, is_within, path_key, year_month_day};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrganizeOptions {
    /// Ex: `{category}/{year}`. Jetons : category, ext, year, month, day, parent.
    pub template: String,
    pub destination: PathBuf,
    /// Ne classe que les fichiers places directement dans le dossier analyse.
    pub top_level_only: bool,
    /// Ne touche jamais aux fichiers d'un projet (git, npm, cargo...).
    pub skip_projects: bool,
    /// Vide = toutes les categories.
    pub categories: HashSet<String>,
}

#[derive(Clone, Debug)]
pub struct MoveItem {
    pub file: FileId,
    pub source: PathBuf,
    pub destination: PathBuf,
}

#[derive(Clone, Debug, Default)]
pub struct OrganizePlan {
    pub moves: Vec<MoveItem>,
    pub skipped_projects: usize,
    pub already_placed: usize,
}

impl OrganizePlan {
    /// Vue arborescente : dossier cible -> (nombre, octets).
    pub fn by_folder(&self, scan: &Scan, root: &Path) -> BTreeMap<String, (u64, u64)> {
        let mut out: BTreeMap<String, (u64, u64)> = BTreeMap::new();
        for item in &self.moves {
            let parent = item.destination.parent().unwrap_or(root);
            let key = parent.strip_prefix(root).unwrap_or(parent).display().to_string();
            let entry = out.entry(key).or_default();
            entry.0 += 1;
            entry.1 += scan.file(item.file).size;
        }
        out
    }
}

fn render(template: &str, scan: &Scan, id: FileId, lang: Lang) -> PathBuf {
    let file = scan.file(id);
    let (year, month, day) = year_month_day(file.modified);
    let ext = scan.ext_of(file);
    let values = [
        ("{category}", category_label(scan.category_of(file), lang).to_string()),
        ("{ext}", if ext.is_empty() { "_".to_string() } else { ext.to_string() }),
        ("{year}", format!("{year:04}")),
        ("{month}", format!("{month:02}")),
        ("{day}", format!("{day:02}")),
        ("{parent}", scan.dir(file.dir).name.to_string()),
    ];
    let mut path = PathBuf::new();
    for segment in template.split(['/', '\\']).filter(|s| !s.trim().is_empty()) {
        let mut text = segment.to_string();
        for (token, value) in &values {
            text = text.replace(token, value);
        }
        // `..` est reduit par safe_segment : le gabarit ne peut pas sortir de la destination.
        path.push(safe_segment(&text));
    }
    path
}

pub fn build_plan(scan: &Scan, root: DirId, options: &OrganizeOptions, lang: Lang) -> OrganizePlan {
    let mut plan = OrganizePlan::default();
    let candidates = if options.top_level_only {
        scan.dir(root).files.clone().filter(|&f| scan.file(f).is_alive()).collect()
    } else {
        scan.files_under(root)
    };

    let mut project_dirs: std::collections::HashMap<DirId, bool> = std::collections::HashMap::new();
    let mut targets: HashSet<String> = HashSet::new();
    for id in candidates {
        let file = scan.file(id);
        if !options.categories.is_empty() && !options.categories.contains(scan.category_of(file)) {
            continue;
        }
        if options.skip_projects
            && *project_dirs.entry(file.dir).or_insert_with(|| is_inside_project(scan, file.dir))
        {
            plan.skipped_projects += 1;
            continue;
        }
        let folder = options.destination.join(render(&options.template, scan, id, lang));
        let source = scan.file_path(id);
        // Deja a sa place, ou dans une arborescence de classement precedente.
        if path_key(&folder) == path_key(scan.folder_of(id)) {
            plan.already_placed += 1;
            continue;
        }
        if is_within(&source, &folder) {
            plan.already_placed += 1;
            continue;
        }
        let mut destination = folder.join(&*file.name);
        // Deux fichiers de meme nom venant de dossiers differents.
        let mut n = 1;
        while !targets.insert(path_key(&destination)) {
            let stem = Path::new(&*file.name).file_stem().unwrap_or_default().to_string_lossy();
            let ext = scan.ext_of(file);
            let dot = if ext.is_empty() { "" } else { "." };
            destination = folder.join(format!("{stem} ({n}){dot}{ext}"));
            n += 1;
        }
        plan.moves.push(MoveItem { file: id, source, destination });
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::model::Progress;
    use crate::scanner::scan;
    use std::fs;

    fn options(dest: &Path, top: bool) -> OrganizeOptions {
        OrganizeOptions {
            template: "{category}/{ext}".into(),
            destination: dest.to_path_buf(),
            top_level_only: top,
            skip_projects: true,
            categories: HashSet::new(),
        }
    }

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        fs::create_dir_all(r.join("sub")).unwrap();
        fs::create_dir_all(r.join("proj")).unwrap();
        fs::write(r.join("a.jpg"), b"1").unwrap();
        fs::write(r.join("sub/a.jpg"), b"2").unwrap();
        fs::write(r.join("proj/package.json"), b"{}").unwrap();
        fs::write(r.join("proj/logo.png"), b"3").unwrap();
        dir
    }

    #[test]
    fn top_level_only_leaves_subfolders_alone() {
        let dir = fixture();
        let s = scan(&[dir.path().to_path_buf()], &Settings::default(), &Progress::default());
        let plan = build_plan(&s, s.root_dirs[0], &options(dir.path(), true), Lang::Fr);
        assert_eq!(plan.moves.len(), 1);
        assert!(plan.moves[0].destination.ends_with("Images/jpg/a.jpg"));
    }

    #[test]
    fn projects_are_protected_and_name_clashes_resolved() {
        let dir = fixture();
        let s = scan(&[dir.path().to_path_buf()], &Settings::default(), &Progress::default());
        let plan = build_plan(&s, s.root_dirs[0], &options(dir.path(), false), Lang::Fr);
        assert_eq!(plan.skipped_projects, 2);
        assert_eq!(plan.moves.len(), 2);
        let names: HashSet<_> = plan.moves.iter().map(|m| m.destination.clone()).collect();
        assert_eq!(names.len(), 2, "destinations distinctes");
    }

    #[test]
    fn rerun_after_classification_is_a_no_op() {
        let dir = fixture();
        fs::create_dir_all(dir.path().join("Images/jpg")).unwrap();
        fs::rename(dir.path().join("a.jpg"), dir.path().join("Images/jpg/a.jpg")).unwrap();
        fs::remove_file(dir.path().join("sub/a.jpg")).unwrap();
        let s = scan(&[dir.path().to_path_buf()], &Settings::default(), &Progress::default());
        let plan = build_plan(&s, s.root_dirs[0], &options(dir.path(), false), Lang::Fr);
        assert!(plan.moves.is_empty());
        assert_eq!(plan.already_placed, 1);
    }

    #[test]
    fn template_cannot_escape_destination() {
        let dir = fixture();
        let s = scan(&[dir.path().to_path_buf()], &Settings::default(), &Progress::default());
        let mut opts = options(dir.path(), true);
        opts.template = "../../{ext}".into();
        let plan = build_plan(&s, s.root_dirs[0], &opts, Lang::Fr);
        assert!(is_within(&plan.moves[0].destination, dir.path()));
    }
}
