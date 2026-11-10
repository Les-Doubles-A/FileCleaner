//! Doublons et images similaires.

use std::path::PathBuf;

use eframe::egui::{
    self, Align, Align2, CornerRadius, Layout, Pos2, Rect, RichText, Sense, Stroke,
    Ui, Vec2,
};
use filecleaner_core::duplicates::{self, DupGroup, DupMode, KeepStrategy};
use filecleaner_core::model::FileId;
use filecleaner_core::util::{format_count, format_date, format_size};

use super::{Page, require_scan};
use crate::app::{Action, App};
use crate::icons;
use crate::jobs::PickPurpose;
use crate::table::{Env, file_icon, file_menu, short_folder};
use crate::theme::{body, category_color, lang, pal, semibold, t};
use crate::widgets::{
    Kind, button, button_enabled, empty_state, notice, page_header, paint_check, paint_text, search_box,
    segmented,
};

#[derive(Clone, Copy)]
pub enum Row {
    Header(usize),
    File(usize, FileId),
}

#[derive(Default)]
pub struct DupesState {
    pub rows: Vec<Row>,
    pub filter: String,
    filter_applied: String,
    pub keep_folder: Option<PathBuf>,
    strategy: usize,
}

impl DupesState {
    pub fn rebuild(&mut self, groups: &[DupGroup]) {
        self.rows.clear();
        for (index, group) in groups.iter().enumerate() {
            self.rows.push(Row::Header(index));
            self.rows.extend(group.files.iter().map(|&f| Row::File(index, f)));
        }
        self.filter_applied.clear();
    }
}

fn strategies(keep_folder: Option<&PathBuf>) -> Vec<KeepStrategy> {
    let mut out = vec![KeepStrategy::Oldest, KeepStrategy::Newest, KeepStrategy::ShortestPath, KeepStrategy::LongestPath];
    if let Some(folder) = keep_folder {
        out.push(KeepStrategy::InFolder(folder.clone()));
    }
    out
}

pub fn duplicates(ui: &mut Ui, app: &mut App) {
    let Some(scan) = require_scan(ui, app) else { return };
    let p = pal(ui);
    let lang = lang();
    page_header(ui, Page::Duplicates.title(), t("Fichiers strictement identiques, vérifiés par empreinte BLAKE3.", "Strictly identical files, verified with BLAKE3 hashes."));

    let busy = app.is_busy();
    ui.horizontal(|ui| {
        let mut mode = app.dup_mode;
        let changed = segmented(ui, &mut mode, &[
            (DupMode::Content, t("Contenu identique", "Same content")),
            (DupMode::ByName, t("Même nom", "Same name")),
        ]);
        if changed && !busy {
            app.actions.push(Action::FindDuplicates(mode));
        }
        if button_enabled(ui, !busy, Kind::Secondary, &format!("{}  {}", icons::ARROWS_CLOCKWISE, t("Relancer", "Run again"))).clicked() {
            app.actions.push(Action::FindDuplicates(app.dup_mode));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            search_box(ui, &mut app.pages.dupes.filter, t("Filtrer par nom", "Filter by name"), 220.0);
        });
    });

    if app.dup_groups.is_empty() {
        if busy {
            empty_state(ui, icons::HOURGLASS, t("Recherche en cours", "Search in progress"), t("Les résultats s'afficheront ici.", "Results will appear here."), None);
        } else if app.dup_done {
            empty_state(ui, icons::CHECK_CIRCLE, t("Aucun doublon", "No duplicates"), t("Chaque fichier est unique.", "Every file is unique."), None);
        } else if empty_state(ui, Page::Duplicates.icon(), t("Recherche non lancée", "Search not started"), t("Seuls les fichiers de même taille sont lus.", "Only files of the same size are read."), Some(t("Chercher les doublons", "Find duplicates"))) {
            app.actions.push(Action::FindDuplicates(DupMode::Content));
        }
        return;
    }

    let identical = app.dup_mode == DupMode::Content;
    if identical {
        ui.horizontal(|ui| {
            let list = strategies(app.pages.dupes.keep_folder.as_ref());
            let index = app.pages.dupes.strategy.min(list.len() - 1);
            let label = |s: &KeepStrategy| match s {
                KeepStrategy::InFolder(folder) => format!("{} {}", t("Garder dans", "Keep in"), folder.display()),
                other => other.label().get(lang).to_string(),
            };
            egui::ComboBox::from_id_salt("keep").width(250.0).selected_text(label(&list[index])).show_ui(ui, |ui| {
                for (i, strategy) in list.iter().enumerate() {
                    if ui.selectable_label(i == index, label(strategy)).clicked() {
                        app.pages.dupes.strategy = i;
                    }
                }
                if ui.button(format!("{}  {}", icons::FOLDER_OPEN, t("Choisir un dossier à garder", "Choose a folder to keep"))).clicked() {
                    app.pages.dupes.strategy = 4;
                    app.actions.push(Action::Pick(PickPurpose::KeepFolder));
                }
            });
            if button(ui, Kind::Primary, &format!("{}  {}", icons::MAGIC_WAND, t("Sélection automatique", "Auto select"))).clicked() {
                for &id in &duplicates::select_redundant(&scan, &app.dup_groups, &list[index]) {
                    app.sel.set(&scan, id, true);
                }
            }
            let recoverable: u64 = app.dup_groups.iter().map(DupGroup::recoverable).sum();
            let text = format!("{} {} · {} {}", format_count(app.dup_groups.len() as u64, lang), t("groupes", "groups"), format_size(recoverable, lang), t("récupérables", "recoverable"));
            ui.label(RichText::new(text).size(13.0).color(p.muted));
        });
    } else {
        notice(ui, icons::WARNING, t(
            "Même nom ne veut pas dire même contenu : vérifiez chaque fichier, aucune sélection automatique ici.",
            "Same name does not mean same content: check each file, no auto selection here.",
        ), p.warn, p.warn_soft);
    }

    let filter = app.pages.dupes.filter.to_lowercase();
    if filter != app.pages.dupes.filter_applied {
        app.pages.dupes.rebuild(&app.dup_groups);
        app.pages.dupes.filter_applied.clone_from(&filter);
        if !filter.is_empty() {
            let groups = &app.dup_groups;
            let keep: Vec<bool> = groups.iter().map(|g| g.files.iter().any(|&f| scan.file(f).name.to_lowercase().contains(&filter))).collect();
            app.pages.dupes.rows.retain(|row| match row {
                Row::Header(g) | Row::File(g, _) => keep[*g],
            });
        }
    }

    let rows = std::mem::take(&mut app.pages.dupes.rows);
    let row_h = 34.0;
    let list = strategies(app.pages.dupes.keep_folder.as_ref());
    let strategy = list[app.pages.dupes.strategy.min(list.len() - 1)].clone();
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, row_h, rows.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for row in &rows[range] {
            let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::click());
            let y = rect.center().y;
            match *row {
                Row::Header(index) => {
                    let group = &app.dup_groups[index];
                    let band = Rect::from_min_max(rect.min + Vec2::new(0.0, 5.0), rect.max);
                    ui.painter().rect_filled(band, CornerRadius::same(6), p.raised);
                    let selected = group.files.iter().filter(|f| app.sel.files.contains(f)).count();
                    let text = format!("{} {} · {}", group.files.len(), t("exemplaires", "copies"), format_size(group.size, lang));
                    let width = paint_text(ui, ui.painter(), Pos2::new(rect.left() + 12.0, y + 2.0), &text, semibold(13.0), p.text, rect.width() - 240.0);
                    if group.physical_copies < group.files.len() {
                        ui.painter().text(Pos2::new(rect.left() + width + 24.0, y + 2.0), Align2::LEFT_CENTER, t("avec liens durs", "with hard links"), body(12.0), p.faint);
                    }
                    if group.identical && selected == group.files.len() {
                        ui.painter().text(Pos2::new(rect.right() - 140.0, y + 2.0), Align2::RIGHT_CENTER, format!("{} {}", icons::WARNING, t("aucune copie gardée", "no copy kept")), body(12.5), p.warn);
                    }
                    let value = format!("-{}", format_size(group.recoverable(), lang));
                    ui.painter().text(Pos2::new(rect.right() - 10.0, y + 2.0), Align2::RIGHT_CENTER, value, semibold(13.0), p.ok);
                }
                Row::File(index, id) => {
                    let file = scan.file(id);
                    let checked = app.sel.files.contains(&id);
                    let focused = app.focus == Some(id);
                    if focused {
                        ui.painter().rect_filled(rect, CornerRadius::same(6), p.accent_soft);
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, CornerRadius::same(6), p.hover);
                    }
                    let on_check = response.hover_pos().is_some_and(|pos| pos.x < rect.left() + 36.0);
                    paint_check(ui, ui.painter(), Pos2::new(rect.left() + 18.0, y), checked, false, on_check);
                    let keeper = identical && duplicates::pick_keeper(&scan, &app.dup_groups[index], &strategy) == id;
                    ui.painter().text(Pos2::new(rect.left() + 46.0, y), Align2::CENTER_CENTER, file_icon(&scan, file), body(16.0), category_color(file.category as usize));
                    let name_w = (rect.width() * 0.3).max(140.0);
                    paint_text(ui, ui.painter(), Pos2::new(rect.left() + 62.0, y), &file.name, body(14.0), p.text, name_w);
                    let folder = short_folder(&scan, scan.folder_of(id));
                    let folder_x = rect.left() + 72.0 + name_w;
                    paint_text(ui, ui.painter(), Pos2::new(folder_x, y), &folder, body(13.0), p.muted, rect.right() - folder_x - 190.0);
                    if keeper {
                        ui.painter().text(Pos2::new(rect.right() - 102.0, y), Align2::RIGHT_CENTER, t("à garder", "keep"), body(12.5), p.ok);
                    }
                    ui.painter().text(Pos2::new(rect.right() - 10.0, y), Align2::RIGHT_CENTER, format_date(file.modified, lang), body(13.0), p.muted);
                    if response.clicked() {
                        if on_check || ui.input(|i| i.modifiers.command) {
                            app.sel.toggle(&scan, id);
                        } else {
                            app.focus = Some(id);
                        }
                    }
                    if response.double_clicked() {
                        app.actions.push(Action::OpenPath(scan.file_path(id)));
                    }
                    let App { sel, focus, actions, .. } = app;
                    let mut env = Env { scan: &scan, sel, focus, actions, row_height: row_h };
                    response.context_menu(|ui| file_menu(ui, &mut env, id));
                }
            }
        }
    });
    app.pages.dupes.rows = rows;
}

pub fn similar(ui: &mut Ui, app: &mut App) {
    let Some(scan) = require_scan(ui, app) else { return };
    let p = pal(ui);
    let lang = lang();
    page_header(ui, Page::Similar.title(), t("Photos proches : recadrages, redimensionnements, rafales.", "Near-identical photos: crops, resizes, bursts."));
    let busy = app.is_busy();
    ui.horizontal(|ui| {
        let label = if app.similar_done { t("Relancer", "Run again") } else { t("Comparer les images", "Compare images") };
        if button_enabled(ui, !busy, Kind::Primary, &format!("{}  {label}", icons::IMAGES)).clicked() {
            app.actions.push(Action::FindSimilar);
        }
        ui.label(RichText::new(t("Tolérance", "Tolerance")).color(p.muted));
        let response = ui.add(egui::Slider::new(&mut app.settings.similar_threshold, 1..=16).show_value(true));
        if response.changed() {
            app.settings_dirty = true;
        }
        if !app.similar.is_empty() {
            if button(ui, Kind::Secondary, &format!("{}  {}", icons::MAGIC_WAND, t("Garder la meilleure résolution", "Keep the best resolution"))).clicked() {
                for group in &app.similar {
                    for item in group.items.iter().skip(1) {
                        app.sel.set(&scan, item.file, true);
                    }
                }
            }
            let recoverable: u64 = app.similar.iter().map(|g| g.recoverable(&scan)).sum();
            let text = format!("{} {} · {}", app.similar.len(), t("groupes", "groups"), format_size(recoverable, lang));
            ui.label(RichText::new(text).size(13.0).color(p.muted));
        }
    });

    if app.similar.is_empty() {
        if app.similar_done {
            empty_state(ui, icons::CHECK_CIRCLE, t("Aucune image similaire", "No similar images"), t("Augmentez la tolérance pour élargir la recherche.", "Raise the tolerance to widen the search."), None);
        } else if !busy {
            empty_state(ui, Page::Similar.icon(), t("Comparaison non lancée", "Comparison not started"), t("Chaque image est décodée : cela peut prendre du temps.", "Every image is decoded, which can take a while."), None);
        }
        return;
    }

    let tile = Vec2::new(168.0, 196.0);
    let row_h = tile.y + 34.0;
    let groups = std::mem::take(&mut app.similar);
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, row_h, groups.len(), |ui, range| {
        for group in &groups[range] {
            ui.allocate_ui(Vec2::new(ui.available_width(), row_h - 8.0), |ui| {
                ui.set_height(row_h - 8.0);
                ui.vertical(|ui| {
                    let text = format!("{} {} · -{}", group.items.len(), t("images", "images"), format_size(group.recoverable(&scan), lang));
                    ui.label(RichText::new(text).font(semibold(13.0)).color(p.text));
                    egui::ScrollArea::horizontal().id_salt(group.items[0].file).auto_shrink([false, true]).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for (position, item) in group.items.iter().enumerate() {
                                let id = item.file;
                                let checked = app.sel.files.contains(&id);
                                let (rect, response) = ui.allocate_exact_size(tile, Sense::click());
                                let stroke = if checked { Stroke::new(2.0, p.accent) } else { Stroke::new(1.0, p.line) };
                                ui.painter().rect(rect, CornerRadius::same(10), p.card, stroke, egui::StrokeKind::Inside);
                                let image_rect = Rect::from_min_size(rect.min + Vec2::splat(6.0), Vec2::new(tile.x - 12.0, 128.0));
                                let path = scan.file_path(id);
                                if let Some(texture) = app.previews.thumbnail(&app.jobs, &path) {
                                    let size = texture.size_vec2();
                                    let scale = (image_rect.width() / size.x).min(image_rect.height() / size.y);
                                    let target = Rect::from_center_size(image_rect.center(), size * scale);
                                    egui::Image::new((texture.id(), size * scale)).corner_radius(6).paint_at(ui, target);
                                } else {
                                    ui.painter().rect_filled(image_rect, CornerRadius::same(6), p.raised);
                                }
                                paint_check(ui, ui.painter(), rect.min + Vec2::splat(20.0), checked, false, response.hovered());
                                let file = scan.file(id);
                                let x = rect.left() + 8.0;
                                paint_text(ui, ui.painter(), Pos2::new(x, rect.top() + 146.0), &file.name, body(12.5), p.text, tile.x - 16.0);
                                let meta = format!("{}x{} · {}", item.width, item.height, format_size(file.size, lang));
                                paint_text(ui, ui.painter(), Pos2::new(x, rect.top() + 163.0), &meta, body(12.0), p.muted, tile.x - 16.0);
                                if position == 0 {
                                    paint_text(ui, ui.painter(), Pos2::new(x, rect.top() + 180.0), t("Meilleure qualité", "Best quality"), body(12.0), p.ok, tile.x - 16.0);
                                }
                                if response.clicked() {
                                    if ui.input(|i| i.modifiers.command) {
                                        app.focus = Some(id);
                                    } else {
                                        app.sel.toggle(&scan, id);
                                    }
                                }
                                if response.double_clicked() {
                                    app.actions.push(Action::OpenPath(path));
                                }
                            }
                        });
                    });
                });
            });
        }
    });
    app.similar = groups;
}
