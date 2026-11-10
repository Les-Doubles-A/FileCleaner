//! Classement automatique et renommage par lots.

use std::collections::HashSet;
use std::path::PathBuf;

use eframe::egui::{self, Align, Align2, Layout, Pos2, RichText, Sense, Ui, Vec2};
use filecleaner_core::actions;
use filecleaner_core::config::{Settings, category_label};
use filecleaner_core::model::{FileId, Scan};
use filecleaner_core::organize::{self, OrganizeOptions, OrganizePlan};
use filecleaner_core::rename::{self, CaseMode, RenameItem, RenameOptions, RenameStatus, SpaceMode};
use filecleaner_core::util::{format_count, format_size};

use super::{Page, require_scan};
use crate::app::{Action, App};
use crate::icons;
use crate::jobs::{JobResult, PickPurpose};
use crate::theme::{body, lang, pal, semibold, t};
use crate::widgets::{
    Kind, button, button_enabled, card, chip, empty_state, notice, page_header, paint_text, section_title,
    segmented, toggle,
};

pub struct OrganizeState {
    template: String,
    top_level_only: bool,
    skip_projects: bool,
    categories: HashSet<String>,
    destination: Option<PathBuf>,
    root: Option<u32>,
    plan: OrganizePlan,
    key: Option<(OrganizeOptions, u64, u32)>,
}

impl OrganizeState {
    pub fn new(settings: &Settings) -> Self {
        Self {
            template: settings.organize_template.clone(),
            top_level_only: true,
            skip_projects: true,
            categories: HashSet::new(),
            destination: None,
            root: None,
            plan: OrganizePlan::default(),
            key: None,
        }
    }

    pub fn on_new_scan(&mut self, _scan: &Scan) {
        self.key = None;
        self.destination = None;
        self.root = None;
    }

    pub fn set_destination(&mut self, path: PathBuf) {
        self.destination = Some(path);
    }
}

pub fn organize(ui: &mut Ui, app: &mut App) {
    let Some(scan) = require_scan(ui, app) else { return };
    let p = pal(ui);
    let lang = lang();
    page_header(ui, Page::Organize.title(), t("Rangez un dossier en sous-dossiers selon vos règles. Rien ne bouge avant validation.", "Sort a folder into subfolders with your rules. Nothing moves until you approve."));

    let root = app.pages.organize.root.unwrap_or(scan.root_dirs[0]);
    let destination = app.pages.organize.destination.clone().unwrap_or_else(|| scan.dir(root).path.clone());
    let options = OrganizeOptions {
        template: app.pages.organize.template.clone(),
        destination: destination.clone(),
        top_level_only: app.pages.organize.top_level_only,
        skip_projects: app.pages.organize.skip_projects,
        categories: app.pages.organize.categories.clone(),
    };
    let key = (options.clone(), scan.generation, root);
    if app.pages.organize.key.as_ref() != Some(&key) {
        app.pages.organize.plan = organize::build_plan(&scan, root, &options, lang);
        app.pages.organize.key = Some(key);
    }

    let width = ui.available_width();
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(Vec2::new((width * 0.42).clamp(320.0, 460.0), ui.available_height()), Layout::top_down(Align::Min), |ui| {
            card(ui).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let state = &mut app.pages.organize;
                section_title(ui, t("Règle de rangement", "Sorting rule"));
                ui.horizontal_wrapped(|ui| {
                    for (label, template) in [
                        (t("Par type", "By type"), "{category}"),
                        (t("Type puis année", "Type then year"), "{category}/{year}"),
                        (t("Par date", "By date"), "{year}/{month}"),
                        (t("Par extension", "By extension"), "{ext}"),
                    ] {
                        if chip(ui, state.template == template, label).clicked() {
                            state.template = template.to_string();
                        }
                    }
                });
                ui.add(egui::TextEdit::singleline(&mut state.template).desired_width(f32::INFINITY));
                ui.label(RichText::new("{category} {ext} {year} {month} {day} {parent}").monospace().size(12.0).color(p.faint));
                ui.add_space(6.0);
                toggle(ui, &mut state.top_level_only, t("Seulement le premier niveau du dossier", "Only the top level of the folder"));
                toggle(ui, &mut state.skip_projects, t("Ne jamais toucher aux projets (git, npm, cargo...)", "Never touch projects (git, npm, cargo...)"));
                ui.add_space(6.0);
                section_title(ui, t("Catégories à ranger", "Categories to sort"));
                ui.horizontal_wrapped(|ui| {
                    if chip(ui, state.categories.is_empty(), t("Toutes", "All")).clicked() {
                        state.categories.clear();
                    }
                    for name in &scan.category_names {
                        if chip(ui, state.categories.contains(name), category_label(name, lang)).clicked() && !state.categories.remove(name) {
                            state.categories.insert(name.clone());
                        }
                    }
                });
                ui.add_space(6.0);
                section_title(ui, t("Destination", "Destination"));
                ui.label(RichText::new(destination.display().to_string()).size(13.0).color(p.muted));
                if button(ui, Kind::Secondary, t("Changer", "Change")).clicked() {
                    app.actions.push(Action::Pick(PickPurpose::OrganizeDestination));
                }
            });
        });

        ui.vertical(|ui| {
            let plan = &app.pages.organize.plan;
            let moves = plan.moves.len();
            let bytes: u64 = plan.moves.iter().map(|m| scan.file(m.file).size).sum();
            ui.horizontal(|ui| {
                let text = format!("{} {} · {}", format_count(moves as u64, lang), t("fichier(s) à déplacer", "file(s) to move"), format_size(bytes, lang));
                ui.label(RichText::new(text).font(semibold(14.0)).color(p.text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let enabled = moves > 0 && !app.jobs.running.is_some();
                    if button_enabled(ui, enabled, Kind::Primary, &format!("{}  {}", icons::TREE_STRUCTURE, t("Appliquer le classement", "Apply sorting"))).clicked() {
                        let pairs: Vec<(PathBuf, PathBuf)> = app.pages.organize.plan.moves.iter().map(|m| (m.source.clone(), m.destination.clone())).collect();
                        let label = t("Classement automatique", "Automatic sorting").to_string();
                        let title = scan.title();
                        app.settings.organize_template.clone_from(&app.pages.organize.template);
                        app.settings_dirty = true;
                        app.jobs.start(t("Classement", "Sorting"), true, move |progress| {
                            JobResult::Moved(actions::move_files(&pairs, &label, &title, progress))
                        });
                    }
                });
            });
            let plan = &app.pages.organize.plan;
            if plan.skipped_projects > 0 {
                let text = format!("{} {}", format_count(plan.skipped_projects as u64, lang), t("fichiers de projets laissés en place.", "project files left in place."));
                ui.label(RichText::new(text).size(12.5).color(p.muted));
            }
            if moves == 0 {
                empty_state(ui, icons::CHECK_CIRCLE, t("Rien à déplacer", "Nothing to move"), t("Tout est déjà rangé selon cette règle.", "Everything already follows this rule."), None);
                return;
            }
            let folders: Vec<(String, (u64, u64))> = plan.by_folder(&scan, &destination).into_iter().collect();
            let max = folders.iter().map(|(_, (_, b))| *b).max().unwrap_or(1).max(1);
            let row_h = 38.0;
            egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, row_h, folders.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (folder, (count, size)) in &folders[range] {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::hover());
                    let y = rect.center().y - 3.0;
                    ui.painter().text(Pos2::new(rect.left() + 14.0, y), Align2::CENTER_CENTER, icons::FOLDER, body(16.0), p.accent);
                    paint_text(ui, ui.painter(), Pos2::new(rect.left() + 32.0, y), folder, body(14.0), p.text, rect.width() - 220.0);
                    let meta = format!("{} {}", format_count(*count, lang), t("fichier(s)", "file(s)"));
                    ui.painter().text(Pos2::new(rect.right() - 86.0, y), Align2::RIGHT_CENTER, meta, body(12.5), p.faint);
                    ui.painter().text(Pos2::new(rect.right() - 6.0, y), Align2::RIGHT_CENTER, format_size(*size, lang), body(13.0), p.muted);
                    let bar = egui::Rect::from_min_size(Pos2::new(rect.left() + 32.0, rect.bottom() - 8.0), Vec2::new(rect.width() - 38.0, 2.0));
                    crate::widgets::meter(ui, ui.painter(), bar, *size as f32 / max as f32, p.accent);
                }
            });
        });
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Selection,
    Category(usize),
    All,
}

pub struct RenameState {
    options: RenameOptions,
    source: Source,
    plan: Vec<RenameItem>,
    error: Option<String>,
    key: Option<(RenameOptions, u64, Vec<FileId>)>,
}

impl RenameState {
    pub fn new(settings: &Settings) -> Self {
        let options = RenameOptions { template: settings.rename_template.clone(), ..RenameOptions::default() };
        Self { options, source: Source::All, plan: Vec::new(), error: None, key: None }
    }
    pub fn invalidate(&mut self) {
        self.key = None;
    }
    pub fn use_selection(&mut self) {
        self.source = Source::Selection;
        self.key = None;
    }
}

fn source_files(app: &App, scan: &Scan) -> Vec<FileId> {
    let mut files: Vec<FileId> = match app.pages.rename.source {
        Source::Selection => app.sel.files.iter().copied().collect(),
        Source::Category(index) => scan.alive().filter(|(_, f)| f.category as usize == index).map(|(id, _)| id).collect(),
        Source::All => scan.alive().map(|(id, _)| id).collect(),
    };
    files.sort_by(|&a, &b| {
        let (fa, fb) = (scan.file(a), scan.file(b));
        fa.dir.cmp(&fb.dir).then_with(|| fa.name.to_lowercase().cmp(&fb.name.to_lowercase()))
    });
    files
}

pub fn rename(ui: &mut Ui, app: &mut App) {
    let Some(scan) = require_scan(ui, app) else { return };
    let p = pal(ui);
    let lang = lang();
    page_header(ui, Page::Rename.title(), t("Composez un modèle et vérifiez le résultat ligne par ligne avant d'appliquer.", "Build a pattern and check each line before applying."));

    let files = source_files(app, &scan);
    let key = (app.pages.rename.options.clone(), scan.generation, files.clone());
    if app.pages.rename.key.as_ref() != Some(&key) {
        match rename::build_plan(&scan, &files, &app.pages.rename.options) {
            Ok(plan) => {
                app.pages.rename.plan = plan;
                app.pages.rename.error = None;
            }
            Err(err) => app.pages.rename.error = Some(err),
        }
        app.pages.rename.key = Some(key);
    }

    let width = ui.available_width();
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(Vec2::new((width * 0.38).clamp(300.0, 420.0), ui.available_height()), Layout::top_down(Align::Min), |ui| {
            ui.set_width((width * 0.38).clamp(300.0, 420.0));
            egui::ScrollArea::vertical().id_salt("rename-options").auto_shrink([false, false]).show(ui, |ui| {
                card(ui).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let selection_count = app.sel.files.len();
                    let state = &mut app.pages.rename;
                    section_title(ui, t("Fichiers concernés", "Files affected"));
                    let label = |s: Source| match s {
                        Source::Selection => format!("{} ({selection_count})", t("Sélection", "Selection")),
                        Source::All => t("Tous les fichiers", "All files").to_string(),
                        Source::Category(i) => category_label(&scan.category_names[i], lang).to_string(),
                    };
                    egui::ComboBox::from_id_salt("rename-source").width(ui.available_width() - 8.0).selected_text(label(state.source)).show_ui(ui, |ui| {
                        for option in [Source::Selection, Source::All] {
                            ui.selectable_value(&mut state.source, option, label(option));
                        }
                        for index in 0..scan.category_names.len() {
                            ui.selectable_value(&mut state.source, Source::Category(index), label(Source::Category(index)));
                        }
                    });
                    ui.add_space(6.0);
                    section_title(ui, t("Modèle", "Pattern"));
                    let o = &mut state.options;
                    ui.horizontal_wrapped(|ui| {
                        for (label, template) in [
                            (t("Nom d'origine", "Original name"), "{name}"),
                            (t("Date + numéro", "Date + number"), "{date}_{n}"),
                            (t("Dossier + numéro", "Folder + number"), "{parent}_{n}"),
                            (t("Date + nom", "Date + name"), "{date} {name}"),
                        ] {
                            if chip(ui, o.template == template, label).clicked() {
                                o.template = template.to_string();
                            }
                        }
                    });
                    ui.add(egui::TextEdit::singleline(&mut o.template).desired_width(f32::INFINITY));
                    ui.label(RichText::new("{name} {n} {date} {mdate} {yyyy} {mm} {dd} {parent}").monospace().size(12.0).color(p.faint));
                    ui.add_space(6.0);
                    section_title(ui, t("Rechercher et remplacer", "Find and replace"));
                    ui.add(egui::TextEdit::singleline(&mut o.find).hint_text(t("Rechercher", "Find")).desired_width(f32::INFINITY));
                    ui.add(egui::TextEdit::singleline(&mut o.replace).hint_text(t("Remplacer par", "Replace with")).desired_width(f32::INFINITY));
                    toggle(ui, &mut o.use_regex, t("Expression régulière", "Regular expression"));
                    ui.add_space(6.0);
                    section_title(ui, t("Mise en forme", "Formatting"));
                    segmented(ui, &mut o.case, &[
                        (CaseMode::Keep, t("Casse d'origine", "Keep case")),
                        (CaseMode::Lower, t("minuscules", "lowercase")),
                        (CaseMode::Upper, t("MAJUSCULES", "UPPERCASE")),
                        (CaseMode::Title, t("Titre", "Title")),
                    ]);
                    segmented(ui, &mut o.spaces, &[
                        (SpaceMode::Keep, t("Espaces", "Spaces")),
                        (SpaceMode::Underscore, t("tiret_bas", "under_score")),
                        (SpaceMode::Dash, t("tiret-simple", "dash-ed")),
                    ]);
                    toggle(ui, &mut o.clean_noise, t("Retirer préfixes photo et « (1) »", "Remove camera prefixes and \"(1)\""));
                    toggle(ui, &mut o.strip_accents, t("Retirer les accents", "Remove accents"));
                    toggle(ui, &mut o.lowercase_ext, t("Extension en minuscules", "Lowercase extension"));
                    toggle(ui, &mut o.counter_per_folder, t("Numéroter par dossier", "Number per folder"));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(t("Début", "Start")).color(p.muted));
                        ui.add(egui::DragValue::new(&mut o.start).range(0..=999_999));
                        ui.label(RichText::new(t("Chiffres", "Digits")).color(p.muted));
                        ui.add(egui::DragValue::new(&mut o.pad).range(1..=8));
                    });
                });
            });
        });

        ui.vertical(|ui| {
            let plan = &app.pages.rename.plan;
            let ready = plan.iter().filter(|i| i.will_apply()).count();
            let invalid = plan.iter().filter(|i| matches!(i.status, RenameStatus::Invalid(_))).count();
            ui.horizontal(|ui| {
                let text = format!("{} {}", format_count(ready as u64, lang), t("fichier(s) à renommer", "file(s) to rename"));
                ui.label(RichText::new(text).font(semibold(14.0)).color(p.text));
                if invalid > 0 {
                    let text = format!("{invalid} {}", t("nom(s) invalide(s), ignoré(s)", "invalid name(s), skipped"));
                    ui.label(RichText::new(text).size(13.0).color(p.danger));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let enabled = ready > 0 && app.pages.rename.error.is_none() && !app.jobs.running.is_some();
                    if button_enabled(ui, enabled, Kind::Primary, &format!("{}  {}", icons::PENCIL_SIMPLE, t("Appliquer", "Apply"))).clicked() {
                        let plan = app.pages.rename.plan.clone();
                        let target = scan.clone();
                        let label = t("Renommage par lots", "Batch rename").to_string();
                        app.settings.rename_template.clone_from(&app.pages.rename.options.template);
                        app.settings_dirty = true;
                        app.jobs.start(t("Renommage", "Renaming"), false, move |_| {
                            JobResult::Renamed(rename::apply_plan(&target, &plan, &label))
                        });
                    }
                });
            });
            if let Some(error) = &app.pages.rename.error {
                let text = format!("{} {error}", t("Expression invalide :", "Invalid expression:"));
                notice(ui, icons::WARNING, &text, p.danger, p.danger_soft);
                return;
            }
            let plan = &app.pages.rename.plan;
            if plan.is_empty() {
                let message = t("Sélectionnez des fichiers ou choisissez une catégorie.", "Select files or pick a category.");
                empty_state(ui, Page::Rename.icon(), t("Aucun fichier", "No files"), message, None);
                return;
            }
            let row_h = 42.0;
            egui::ScrollArea::vertical().id_salt("rename-rows").auto_shrink(false).show_rows(ui, row_h, plan.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for item in &plan[range] {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::hover());
                    ui.painter().hline(rect.x_range(), rect.bottom(), egui::Stroke::new(1.0, p.line));
                    let (icon, color, note) = match &item.status {
                        RenameStatus::Ready => (icons::ARROW_RIGHT, p.accent, None),
                        RenameStatus::Unchanged => (icons::MINUS, p.faint, None),
                        RenameStatus::Adjusted => (icons::WARNING, p.warn, Some(t("Numéro ajouté pour éviter un conflit", "Number added to avoid a conflict").to_string())),
                        RenameStatus::Invalid(reason) => (icons::X_CIRCLE, p.danger, Some(reason.get(lang).to_string())),
                    };
                    ui.painter().text(rect.left_center() + Vec2::new(12.0, 0.0), Align2::CENTER_CENTER, icon, body(15.0), color);
                    let x = rect.left() + 30.0;
                    let w = rect.width() - 40.0;
                    paint_text(ui, ui.painter(), Pos2::new(x, rect.top() + 13.0), &item.old_name, body(12.5), p.muted, w);
                    let text_color = if item.status == RenameStatus::Unchanged { p.faint } else { p.text };
                    let shown = match &note {
                        Some(note) => format!("{}   ({note})", item.new_name),
                        None => item.new_name.clone(),
                    };
                    paint_text(ui, ui.painter(), Pos2::new(x, rect.top() + 30.0), &shown, body(14.0), text_color, w);
                }
            });
        });
    });
}
