//! Pages listes : recherche, inutiles, gros, anciens, rares, vides, artefacts.

use eframe::egui::{
    self, Align, Align2, CornerRadius, Layout, Pos2, Rect, RichText, Sense, Ui, Vec2,
};
use filecleaner_core::actions;
use filecleaner_core::analysis::{self, RULE_COUNT, Risk, rule_label, rule_risk};
use filecleaner_core::model::{FileId, Scan};
use filecleaner_core::search::Query;
use filecleaner_core::util::{format_age, format_count, format_size};

use super::{Page, require_scan};
use crate::app::{Action, App};
use crate::dialogs::ToastKind;
use crate::icons;
use crate::jobs::JobResult;
use crate::table::{SortKey, TableState, file_table, short_folder};
use crate::theme::{body, lang, pal, semibold, t};
use crate::widgets::{
    Kind, button, button_enabled, chip, empty_state, notice, number, page_header, paint_check,
    paint_text, search_box,
};

fn total_bytes(scan: &Scan, rows: &[FileId]) -> u64 {
    rows.iter().map(|&id| scan.file(id).size).sum()
}

fn summary(rows: usize, bytes: u64) -> String {
    format!("{} {} · {}", format_count(rows as u64, lang()), t("fichier(s)", "file(s)"), format_size(bytes, lang()))
}

/// Filtre rapide commun aux listes.
fn quick_filter(ui: &mut Ui, app: &mut App, page: Page) {
    let state = app.tables.entry(page).or_default();
    search_box(ui, &mut state.filter, t("Filtrer (nom, ext:pdf, size:>1mb...)", "Filter (name, ext:pdf, size:>1mb...)"), 280.0);
}

fn show_table(ui: &mut Ui, app: &mut App, page: Page, scan: &Scan, rows: &[FileId]) {
    let App { tables, sel, focus, actions, settings, .. } = app;
    let state = tables.entry(page).or_default();
    let mut env = App::env(scan, sel, focus, actions, settings.compact_rows);
    file_table(ui, &mut env, state, rows, None, None);
}

fn refresh_threshold_rows(app: &mut App, scan: &Scan) {
    let pages = &mut app.pages;
    if pages.rows_generation != scan.generation {
        pages.rows_generation = scan.generation;
        pages.big_rows = analysis::find_big(scan, pages.big_mb * 1024 * 1024);
        pages.old_rows = analysis::find_old(scan, pages.old_days);
        pages.rare_rows = analysis::find_rare_exts(scan, pages.rare_max);
    }
}

pub fn files(ui: &mut Ui, app: &mut App) {
    let Some(scan) = require_scan(ui, app) else { return };
    let p = pal(ui);
    page_header(ui, Page::Files.title(), t("Tous les fichiers analysés, avec des filtres combinables.", "All scanned files, with combinable filters."));
    let width = (ui.available_width() - 2.0).min(720.0);
    let response = search_box(ui, &mut app.pages.search, t("rapport ext:pdf size:>10mb age:>365 in:projets -brouillon", "report ext:pdf size:>10mb age:>365 in:projects -draft"), width);
    if app.pages.focus_search {
        app.pages.focus_search = false;
        response.request_focus();
    }
    ui.horizontal_wrapped(|ui| {
        for (label, query) in [
            (t("Images", "Images"), "cat:images"),
            (t("Vidéos", "Videos"), "cat:vid"),
            (t("Documents", "Documents"), "cat:documents"),
            (t("Archives", "Archives"), "cat:archives"),
            (t("Plus de 100 Mo", "Over 100 MB"), "size:>100mb"),
            (t("Récents (30 j)", "Recent (30 d)"), "age:<30"),
            (t("Plus de 2 ans", "Over 2 years"), "age:>730"),
            (t("Masqués", "Hidden"), "is:hidden"),
        ] {
            let active = app.pages.search.contains(query);
            if chip(ui, active, label).clicked() {
                let search = &mut app.pages.search;
                if active {
                    *search = search.replace(query, "").trim().to_string();
                } else {
                    if !search.is_empty() {
                        search.push(' ');
                    }
                    search.push_str(query);
                }
            }
        }
    });

    let key = (app.pages.search.clone(), scan.generation);
    if app.pages.search_key.as_ref() != Some(&key) {
        let query = Query::parse(&app.pages.search);
        app.pages.search_rows = query.run(&scan);
        app.pages.search_problems = query.problems;
        app.pages.search_key = Some(key);
    }
    if !app.pages.search_problems.is_empty() {
        let text = format!("{} {}", t("Filtre non compris :", "Filter not understood:"), app.pages.search_problems.join(", "));
        notice(ui, icons::WARNING, &text, p.warn, p.warn_soft);
    }
    let rows = std::mem::take(&mut app.pages.search_rows);
    ui.label(RichText::new(summary(rows.len(), total_bytes(&scan, &rows))).size(13.0).color(p.muted));
    show_table(ui, app, Page::Files, &scan, &rows);
    app.pages.search_rows = rows;
}

pub fn big(ui: &mut Ui, app: &mut App) {
    let Some(scan) = require_scan(ui, app) else { return };
    refresh_threshold_rows(app, &scan);
    let p = pal(ui);
    page_header(ui, Page::Big.title(), t("Les fichiers qui pèsent le plus, du plus gros au plus petit.", "The heaviest files, largest first."));
    ui.horizontal(|ui| {
        ui.label(RichText::new(t("À partir de", "From")).color(p.muted));
        let mut changed = false;
        for (mb, label) in [
            (10, t("10 Mo", "10 MB")),
            (100, t("100 Mo", "100 MB")),
            (500, t("500 Mo", "500 MB")),
            (1024, t("1 Go", "1 GB")),
            (5120, t("5 Go", "5 GB")),
        ] {
            if chip(ui, app.pages.big_mb == mb, label).clicked() {
                app.pages.big_mb = mb;
                changed = true;
            }
        }
        changed |= number(ui, &mut app.pages.big_mb, 1..=1_000_000, t(" Mo", " MB")).changed();
        if changed {
            app.pages.rows_generation = 0;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| quick_filter(ui, app, Page::Big));
    });
    let rows = std::mem::take(&mut app.pages.big_rows);
    ui.label(RichText::new(summary(rows.len(), total_bytes(&scan, &rows))).size(13.0).color(p.muted));
    if rows.is_empty() {
        empty_state(ui, Page::Big.icon(), t("Aucun gros fichier", "No large files"), t("Baissez le seuil pour en voir davantage.", "Lower the threshold to see more."), None);
    } else {
        show_table(ui, app, Page::Big, &scan, &rows);
    }
    app.pages.big_rows = rows;
}

pub fn old(ui: &mut Ui, app: &mut App) {
    let Some(scan) = require_scan(ui, app) else { return };
    refresh_threshold_rows(app, &scan);
    app.tables.entry(Page::Old).or_insert_with(|| TableState::sorted_by(SortKey::Modified, false));
    let p = pal(ui);
    page_header(ui, Page::Old.title(), t("Fichiers qui n'ont pas été modifiés depuis longtemps.", "Files that have not changed in a long time."));
    ui.horizontal(|ui| {
        ui.label(RichText::new(t("Sans modification depuis", "Unchanged for")).color(p.muted));
        for (days, label) in [
            (180, t("6 mois", "6 months")),
            (365, t("1 an", "1 year")),
            (730, t("2 ans", "2 years")),
            (1825, t("5 ans", "5 years")),
            (3650, t("10 ans", "10 years")),
        ] {
            if chip(ui, app.pages.old_days == days, label).clicked() {
                app.pages.old_days = days;
                app.pages.rows_generation = 0;
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| quick_filter(ui, app, Page::Old));
    });
    let rows = std::mem::take(&mut app.pages.old_rows);
    ui.label(RichText::new(summary(rows.len(), total_bytes(&scan, &rows))).size(13.0).color(p.muted));
    if rows.is_empty() {
        empty_state(ui, Page::Old.icon(), t("Aucun fichier ancien", "No old files"), t("Rien ne correspond à cette durée.", "Nothing matches this period."), None);
    } else {
        show_table(ui, app, Page::Old, &scan, &rows);
    }
    app.pages.old_rows = rows;
}

pub fn rare(ui: &mut Ui, app: &mut App) {
    let Some(scan) = require_scan(ui, app) else { return };
    refresh_threshold_rows(app, &scan);
    let p = pal(ui);
    page_header(ui, Page::Rare.title(), t("Types de fichiers isolés, souvent oubliés ou mal rangés.", "Isolated file types, often forgotten or misplaced."));
    ui.horizontal(|ui| {
        ui.label(RichText::new(t("Occurrences maximum", "Maximum occurrences")).color(p.muted));
        let mut value = u64::from(app.pages.rare_max);
        if number(ui, &mut value, 1..=50, "").changed() {
            app.pages.rare_max = value as u32;
            app.pages.rows_generation = 0;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| quick_filter(ui, app, Page::Rare));
    });
    let rows = std::mem::take(&mut app.pages.rare_rows);
    ui.label(RichText::new(summary(rows.len(), total_bytes(&scan, &rows))).size(13.0).color(p.muted));
    show_table(ui, app, Page::Rare, &scan, &rows);
    app.pages.rare_rows = rows;
}

pub fn junk(ui: &mut Ui, app: &mut App) {
    let Some(scan) = require_scan(ui, app) else { return };
    let p = pal(ui);
    let lang = lang();
    page_header(ui, Page::Junk.title(), t("Temporaires, caches, journaux et résidus.", "Temporary files, caches, logs and leftovers."));
    let Some(derived) = app.derived.clone() else {
        ui.spinner();
        return;
    };
    if derived.junk.is_empty() {
        empty_state(ui, icons::CHECK_CIRCLE, t("Rien à signaler", "Nothing to report"), t("Aucun fichier inutile détecté ici.", "No junk files detected here."), None);
        return;
    }

    let mut stats = [(0u64, 0u64); RULE_COUNT];
    for hit in &derived.junk {
        stats[hit.rule].0 += 1;
        stats[hit.rule].1 += scan.file(hit.file).size;
    }
    ui.horizontal_wrapped(|ui| {
        let all = format!("{} ({})", t("Tout", "All"), derived.junk.len());
        if chip(ui, app.pages.junk_filter.is_none(), &all).clicked() {
            app.pages.junk_filter = None;
        }
        for (rule, (count, bytes)) in stats.iter().enumerate() {
            if *count == 0 {
                continue;
            }
            let dot = if rule_risk(rule) == Risk::Safe { icons::SHIELD_CHECK } else { icons::EYE };
            let label = format!("{dot} {} · {}", rule_label(rule).get(lang), format_size(*bytes, lang));
            if chip(ui, app.pages.junk_filter == Some(rule), &label).clicked() {
                app.pages.junk_filter = Some(rule);
            }
        }
    });
    let key = (app.pages.junk_filter, derived.generation);
    if app.pages.junk_key != Some(key) {
        app.pages.junk_key = Some(key);
        let filter = app.pages.junk_filter;
        app.pages.junk_rows = derived.junk.iter().filter(|h| filter.is_none_or(|f| h.rule == f)).map(|h| h.file).collect();
    }
    ui.horizontal(|ui| {
        let safe = format!("{}  {}", icons::SHIELD_CHECK, t("Sélectionner le sans risque", "Select risk-free"));
        if button(ui, Kind::Secondary, &safe).clicked() {
            for hit in derived.junk.iter().filter(|h| rule_risk(h.rule) == Risk::Safe) {
                app.sel.set(&scan, hit.file, true);
            }
        }
        ui.label(RichText::new(icons::SHIELD_CHECK).color(p.ok));
        ui.label(RichText::new(t("sans risque", "risk-free")).size(12.5).color(p.muted));
        ui.label(RichText::new(icons::EYE).color(p.warn));
        ui.label(RichText::new(t("à vérifier", "to review")).size(12.5).color(p.muted));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| quick_filter(ui, app, Page::Junk));
    });

    let rows = std::mem::take(&mut app.pages.junk_rows);
    {
        let rules = &derived.junk_rules;
        let tag = |id: FileId| rules.get(&id).map_or_else(String::new, |r| rule_label(*r).get(lang).to_string());
        let App { tables, sel, focus, actions, settings, .. } = app;
        let state = tables.entry(Page::Junk).or_default();
        let mut env = App::env(&scan, sel, focus, actions, settings.compact_rows);
        file_table(ui, &mut env, state, &rows, Some(t("Type", "Type")), Some(&tag));
    }
    app.pages.junk_rows = rows;
}

pub fn empty(ui: &mut Ui, app: &mut App) {
    let Some(scan) = require_scan(ui, app) else { return };
    let p = pal(ui);
    page_header(ui, Page::Empty.title(), t("Dossiers sans aucun fichier, sous-dossiers compris.", "Folders without any file, subfolders included."));
    let Some(derived) = app.derived.clone() else {
        ui.spinner();
        return;
    };
    if app.pages.empty_generation != derived.generation {
        app.pages.empty_generation = derived.generation;
        app.pages.empty_selected = derived.empty.iter().copied().collect();
    }
    if derived.empty.is_empty() {
        empty_state(ui, icons::CHECK_CIRCLE, t("Aucun dossier vide", "No empty folders"), t("Tout est en ordre.", "Everything is in order."), None);
        return;
    }
    ui.horizontal(|ui| {
        let n = app.pages.empty_selected.len();
        let text = format!("{}  {} {}", icons::TRASH, t("Retirer", "Remove"), n);
        if button_enabled(ui, n > 0 && !app.jobs.running.is_some(), Kind::Primary, &text).clicked() {
            let dirs: Vec<u32> = app.pages.empty_selected.iter().copied().collect();
            let target = scan.clone();
            let label = t("Dossiers vides", "Empty folders").to_string();
            let language = lang();
            app.jobs.start(t("Retrait des dossiers vides", "Removing empty folders"), false, move |_| {
                JobResult::EmptyDirs(actions::remove_empty_dirs(&target, &dirs, &label, language))
            });
        }
        if button(ui, Kind::Ghost, t("Tout cocher", "Select all")).clicked() {
            app.pages.empty_selected = derived.empty.iter().copied().collect();
        }
        if button(ui, Kind::Ghost, t("Tout décocher", "Clear")).clicked() {
            app.pages.empty_selected.clear();
        }
        ui.label(RichText::new(t("Seuls des dossiers réellement vides peuvent être retirés.", "Only truly empty folders can be removed.")).size(12.5).color(p.muted));
    });
    let row_h = 34.0;
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, row_h, derived.empty.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for &dir in &derived.empty[range] {
            let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(rect, CornerRadius::same(6), p.hover);
            }
            let checked = app.pages.empty_selected.contains(&dir);
            paint_check(ui, ui.painter(), rect.left_center() + Vec2::new(18.0, 0.0), checked, false, response.hovered());
            ui.painter().text(rect.left_center() + Vec2::new(46.0, 0.0), Align2::CENTER_CENTER, icons::FOLDER_DASHED, body(16.0), p.muted);
            let path = short_folder(&scan, &scan.dir(dir).path);
            paint_text(ui, ui.painter(), rect.left_center() + Vec2::new(62.0, 0.0), &path, body(13.5), p.text, rect.width() - 80.0);
            if response.clicked() {
                if checked {
                    app.pages.empty_selected.remove(&dir);
                } else {
                    app.pages.empty_selected.insert(dir);
                }
            }
            let target = scan.dir(dir).path.clone();
            response.context_menu(|ui| {
                if ui.button(t("Afficher dans l'Explorateur", "Show in Explorer")).clicked() {
                    app.actions.push(Action::Reveal(target.clone()));
                }
            });
        }
    });
}

pub fn dev(ui: &mut Ui, app: &mut App) {
    let Some(scan) = require_scan(ui, app) else { return };
    let p = pal(ui);
    let lang = lang();
    page_header(ui, Page::Dev.title(), t("Dossiers régénérables par vos outils : node_modules, target, .venv...", "Folders your tools can rebuild: node_modules, target, .venv..."));
    let Some(derived) = app.derived.clone() else {
        ui.spinner();
        return;
    };
    if derived.dev.is_empty() {
        empty_state(ui, Page::Dev.icon(), t("Aucun artefact", "No artifacts"), t("Aucun dossier de build ou de dépendances détecté.", "No build or dependency folder detected."), None);
        return;
    }
    ui.horizontal(|ui| {
        let total: u64 = derived.dev.iter().map(|a| a.size).sum();
        let text = format!("{} {} · {}", derived.dev.len(), t("dossier(s)", "folder(s)"), format_size(total, lang));
        ui.label(RichText::new(text).size(13.0).color(p.muted));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = t("Sélectionner les projets inactifs depuis 90 jours", "Select projects idle for 90 days");
            if button(ui, Kind::Secondary, label).clicked() {
                let limit = filecleaner_core::util::now_ts() - 90 * 86_400;
                let mut added = 0;
                for artifact in derived.dev.iter().filter(|a| a.last_modified < limit) {
                    if !app.sel.dirs.contains(&artifact.dir) {
                        app.sel.toggle_dir(&scan, artifact.dir);
                        added += 1;
                    }
                }
                if added == 0 {
                    app.toast(ToastKind::Info, t("Aucun projet inactif.", "No idle project."));
                }
            }
        });
    });
    let row_h = 52.0;
    let max = derived.dev.first().map_or(1, |a| a.size).max(1);
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, row_h, derived.dev.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for artifact in &derived.dev[range] {
            let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(rect, CornerRadius::same(8), p.hover);
            }
            let dir = scan.dir(artifact.dir);
            let checked = app.sel.dirs.contains(&artifact.dir);
            paint_check(ui, ui.painter(), rect.left_center() + Vec2::new(18.0, 0.0), checked, false, response.hovered());
            let x = rect.left() + 42.0;
            let title = format!("{} / {}", artifact.project, dir.name);
            let title_width = paint_text(ui, ui.painter(), Pos2::new(x, rect.top() + 17.0), &title, semibold(14.0), p.text, rect.width() - 330.0);
            let tag_rect = Rect::from_min_size(Pos2::new(x + title_width + 8.0, rect.top() + 8.0), Vec2::new(0.0, 18.0));
            ui.painter().text(tag_rect.left_center(), Align2::LEFT_CENTER, artifact.kind, body(12.0), p.accent);
            paint_text(ui, ui.painter(), Pos2::new(x, rect.top() + 36.0), &short_folder(&scan, &dir.path), body(12.5), p.muted, rect.width() - 300.0);
            let right = rect.right() - 10.0;
            ui.painter().text(Pos2::new(right, rect.top() + 17.0), Align2::RIGHT_CENTER, format_size(artifact.size, lang), semibold(14.0), p.text);
            let meta = format!("{} {} · {}", format_count(u64::from(artifact.files), lang), t("fichiers", "files"), format_age(artifact.last_modified, lang));
            ui.painter().text(Pos2::new(right, rect.top() + 36.0), Align2::RIGHT_CENTER, meta, body(12.5), p.faint);
            let bar = Rect::from_min_size(Pos2::new(x, rect.bottom() - 3.0), Vec2::new(rect.width() - 52.0, 2.0));
            crate::widgets::meter(ui, ui.painter(), bar, artifact.size as f32 / max as f32, p.accent);
            if response.clicked() {
                app.sel.toggle_dir(&scan, artifact.dir);
            }
            let path = dir.path.clone();
            response.context_menu(|ui| {
                if ui.button(t("Afficher dans l'Explorateur", "Show in Explorer")).clicked() {
                    app.actions.push(Action::Reveal(path.clone()));
                }
            });
        }
    });
}
