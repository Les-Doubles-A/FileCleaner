//! Nettoyage des emplacements Windows : caches, temporaires, corbeille.

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, Align, Align2, CornerRadius, Layout, Pos2, RichText, Sense, Ui, Vec2};
use filecleaner_core::actions::DeleteMode;
use filecleaner_core::system::{self, Measure, SystemTarget};
use filecleaner_core::util::{format_count, format_size};

use super::Page;
use crate::app::App;
use crate::dialogs::Review;
use crate::icons;
use crate::jobs::JobResult;
use crate::theme::{body, lang, pal, semibold, t};
use crate::widgets::{Kind, button, button_enabled, card, notice, page_header, paint_check, paint_text};

#[derive(Default)]
pub struct SystemState {
    pub needs_measure: bool,
    pub targets: Vec<SystemTarget>,
    pub measured: HashMap<&'static str, Measure>,
    pub selected: HashSet<&'static str>,
    pub recycle: Option<(u64, u64)>,
    pub confirm_empty: bool,
    initialized: bool,
}

fn measure_all(app: &mut App) {
    let state = &mut app.pages.system;
    state.needs_measure = false;
    state.measured.clear();
    state.targets = system::system_targets();
    if !state.initialized {
        state.initialized = true;
        state.selected = state.targets.iter().filter(|t| t.preselected).map(|t| t.id).collect();
    }
    for target in state.targets.clone() {
        app.jobs.quiet(move || {
            let measure = system::measure(&target);
            JobResult::Measured(target.id, measure)
        });
    }
}

pub fn show(ui: &mut Ui, app: &mut App) {
    if app.pages.system.needs_measure || app.pages.system.targets.is_empty() {
        measure_all(app);
    }
    let p = pal(ui);
    let lang = lang();
    page_header(ui, Page::System.title(), t("Caches et temporaires hors de vos documents.", "Caches and temporary files outside your documents."));

    let state = &app.pages.system;
    let selected_bytes: u64 = state.selected.iter().filter_map(|id| state.measured.get(id)).map(|m| m.size).sum();
    let measuring = state.measured.len() < state.targets.len();
    ui.horizontal(|ui| {
        let text = format!("{}  {} {}", icons::BROOM, t("Nettoyer", "Clean up"), format_size(selected_bytes, lang));
        let enabled = selected_bytes > 0 && !app.jobs.running.is_some();
        if button_enabled(ui, enabled, Kind::Primary, &text).clicked() {
            let state = &app.pages.system;
            let targets: Vec<_> = state
                .selected
                .iter()
                .filter_map(|id| state.measured.get(id))
                .flat_map(Measure::targets)
                .collect();
            let warnings = vec![t(
                "Les éléments utilisés par une application ouverte seront ignorés. Avec la Corbeille, l'espace n'est libéré qu'après l'avoir vidée.",
                "Items used by an open application are skipped. With the Recycle Bin, space is only freed once it is emptied.",
            )
            .to_string()];
            app.review = Some(Review::new(
                t("Nettoyage Windows", "Windows cleanup").to_string(),
                targets,
                DeleteMode::Trash,
                warnings,
            ));
        }
        if button(ui, Kind::Secondary, &format!("{}  {}", icons::ARROWS_CLOCKWISE, t("Actualiser", "Refresh"))).clicked() {
            app.pages.system.needs_measure = true;
            app.request_recycle_stats();
        }
        if measuring {
            ui.spinner();
            ui.label(RichText::new(t("Mesure en cours...", "Measuring...")).size(13.0).color(p.muted));
        }
    });

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        let targets = app.pages.system.targets.clone();
        for target in &targets {
            let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 54.0), Sense::click());
            let state = &mut app.pages.system;
            let selected = state.selected.contains(target.id);
            let fill = if response.hovered() { p.hover } else { p.card };
            ui.painter().rect_filled(rect, CornerRadius::same(10), fill);
            paint_check(ui, ui.painter(), rect.left_center() + Vec2::new(20.0, 0.0), selected, false, response.hovered());
            let x = rect.left() + 44.0;
            paint_text(ui, ui.painter(), Pos2::new(x, rect.top() + 18.0), target.label.get(lang), semibold(14.0), p.text, rect.width() - 200.0);
            paint_text(ui, ui.painter(), Pos2::new(x, rect.top() + 37.0), target.hint.get(lang), body(12.5), p.muted, rect.width() - 200.0);
            let right = rect.right() - 16.0;
            match state.measured.get(target.id) {
                Some(measure) => {
                    let color = if measure.size > 0 { p.text } else { p.faint };
                    ui.painter().text(Pos2::new(right, rect.top() + 18.0), Align2::RIGHT_CENTER, format_size(measure.size, lang), semibold(14.0), color);
                    let files = format!("{} {}", format_count(measure.files, lang), t("fichiers", "files"));
                    ui.painter().text(Pos2::new(right, rect.top() + 37.0), Align2::RIGHT_CENTER, files, body(12.5), p.faint);
                }
                None => {
                    ui.painter().text(Pos2::new(right, rect.center().y), Align2::RIGHT_CENTER, icons::HOURGLASS, body(16.0), p.faint);
                }
            }
            if response.clicked() {
                if selected {
                    state.selected.remove(target.id);
                } else {
                    state.selected.insert(target.id);
                }
            }
        }

        ui.add_space(8.0);
        card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(icons::RECYCLE).size(22.0).color(p.accent));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(RichText::new(t("Corbeille Windows", "Recycle Bin")).font(semibold(14.0)).color(p.text));
                    let text = match app.pages.system.recycle {
                        Some((items, bytes)) => format!("{} {} · {}", format_count(items, lang), t("éléments", "items"), format_size(bytes, lang)),
                        None => t("Lecture...", "Reading...").to_string(),
                    };
                    ui.label(RichText::new(text).size(12.5).color(p.muted));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let confirm = app.pages.system.confirm_empty;
                    let empty = app.pages.system.recycle.is_none_or(|(items, _)| items == 0);
                    if confirm {
                        if button(ui, Kind::Danger, t("Confirmer, vider", "Confirm, empty")).clicked() {
                            app.pages.system.confirm_empty = false;
                            app.jobs.start(t("Vidage de la corbeille", "Emptying the Recycle Bin"), false, |_| {
                                JobResult::RecycleEmptied(system::empty_recycle_bin().map_err(|err| err.to_string()))
                            });
                        }
                        if button(ui, Kind::Secondary, t("Annuler", "Cancel")).clicked() {
                            app.pages.system.confirm_empty = false;
                        }
                    } else if button_enabled(ui, !empty, Kind::Secondary, t("Vider la corbeille", "Empty the Recycle Bin")).clicked() {
                        app.pages.system.confirm_empty = true;
                    }
                });
            });
            if app.pages.system.confirm_empty {
                notice(ui, icons::WARNING, t("Vider la corbeille est irréversible.", "Emptying the Recycle Bin cannot be undone."), p.danger, p.danger_soft);
            }
        });
    });
}
