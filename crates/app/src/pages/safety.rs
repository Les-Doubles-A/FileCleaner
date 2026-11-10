//! Quarantaine et historique des operations.

use std::path::PathBuf;

use eframe::egui::{self, Align, Align2, Layout, Pos2, RichText, Sense, Ui, Vec2};
use filecleaner_core::history::{OpKind, Operation};
use filecleaner_core::util::{format_age, format_count, format_datetime, format_size};

use super::Page;
use crate::app::{Action, App};
use crate::dialogs::ToastKind;
use crate::icons;
use crate::theme::{body, lang, pal, semibold, t};
use crate::widgets::{Kind, badge, button, button_enabled, card, empty_state, page_header, paint_text};

pub fn quarantine(ui: &mut Ui, app: &mut App) {
    let p = pal(ui);
    let lang = lang();
    page_header(ui, Page::Quarantine.title(), t("Fichiers mis de côté. Ils occupent encore de l'espace tant qu'ils ne sont pas purgés.", "Files set aside. They still use space until purged."));

    let items: Vec<(String, i64, PathBuf, u64, bool)> = app
        .history
        .quarantined()
        .map(|(s, o)| (s.id.clone(), s.timestamp, o.source.clone(), o.size, o.is_dir))
        .collect();
    if items.is_empty() {
        empty_state(ui, Page::Quarantine.icon(), t("Quarantaine vide", "Quarantine is empty"), t("Choisissez « Quarantaine locale » lors d'un nettoyage pour y placer des fichiers.", "Choose \"Local quarantine\" during a cleanup to put files here."), None);
        return;
    }

    ui.horizontal(|ui| {
        let total: u64 = items.iter().map(|i| i.3).sum();
        let text = format!("{} {} · {}", format_count(items.len() as u64, lang), t("élément(s)", "item(s)"), format_size(total, lang));
        ui.label(RichText::new(text).color(p.muted));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if app.pages.settings.confirm_purge {
                if button(ui, Kind::Danger, t("Confirmer la purge définitive", "Confirm permanent purge")).clicked() {
                    app.pages.settings.confirm_purge = false;
                    let freed = app.history.purge_quarantine(None);
                    let text = format!("{} {}", format_size(freed, lang), t("libérés", "freed"));
                    app.toast(ToastKind::Success, text);
                }
                if button(ui, Kind::Secondary, t("Annuler", "Cancel")).clicked() {
                    app.pages.settings.confirm_purge = false;
                }
            } else if button(ui, Kind::Secondary, &format!("{}  {}", icons::TRASH, t("Tout purger", "Purge all"))).clicked() {
                app.pages.settings.confirm_purge = true;
            }
        });
    });

    let row_h = 48.0;
    let mut restore = None;
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, row_h, items.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (session, timestamp, source, size, is_dir) in &items[range] {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::hover());
            let icon = if *is_dir { icons::FOLDER } else { icons::FILE };
            ui.painter().text(rect.left_center() + Vec2::new(16.0, 0.0), Align2::CENTER_CENTER, icon, body(18.0), p.muted);
            let name = source.file_name().map_or_else(|| source.display().to_string(), |n| n.to_string_lossy().into_owned());
            let x = rect.left() + 38.0;
            let w = rect.width() - 260.0;
            paint_text(ui, ui.painter(), Pos2::new(x, rect.top() + 15.0), &name, semibold(14.0), p.text, w);
            paint_text(ui, ui.painter(), Pos2::new(x, rect.top() + 33.0), &source.to_string_lossy(), body(12.5), p.muted, w);
            ui.painter().text(Pos2::new(rect.right() - 128.0, rect.top() + 15.0), Align2::RIGHT_CENTER, format_size(*size, lang), body(13.0), p.text);
            ui.painter().text(Pos2::new(rect.right() - 128.0, rect.top() + 33.0), Align2::RIGHT_CENTER, format_age(*timestamp, lang), body(12.5), p.faint);
            let button_rect = egui::Rect::from_min_size(Pos2::new(rect.right() - 112.0, rect.center().y - 16.0), Vec2::new(108.0, 32.0));
            let response = ui.put(button_rect, egui::Button::new(t("Restaurer", "Restore")));
            if response.clicked() {
                restore = Some((session.clone(), source.clone()));
            }
        }
    });
    if let Some((session, source)) = restore {
        match app.history.restore_quarantined(&session, &source) {
            Ok(()) => {
                app.toast(ToastKind::Success, t("Fichier restauré.", "File restored."));
                app.actions.push(Action::Rescan);
            }
            Err(err) => app.toast(ToastKind::Error, format!("{err:#}")),
        }
    }
}

fn kind_label(op: &Operation) -> &'static str {
    match op.kind {
        OpKind::Trash => t("Corbeille", "Recycle Bin"),
        OpKind::Quarantine => t("Quarantaine", "Quarantine"),
        OpKind::Permanent => t("Définitif", "Permanent"),
        OpKind::Move => t("Déplacé", "Moved"),
        OpKind::Rename => t("Renommé", "Renamed"),
        OpKind::RemoveDir => t("Dossier retiré", "Folder removed"),
    }
}

pub fn history(ui: &mut Ui, app: &mut App) {
    let p = pal(ui);
    let lang = lang();
    page_header(ui, Page::History.title(), t("Toutes les opérations, restaurables tant que les fichiers existent.", "All operations, restorable as long as the files exist."));
    if app.history.sessions.is_empty() {
        empty_state(ui, Page::History.icon(), t("Aucune opération", "No operations"), t("Vos nettoyages apparaîtront ici.", "Your cleanups will appear here."), None);
        return;
    }
    ui.horizontal(|ui| {
        if button(ui, Kind::Ghost, t("Effacer l'historique", "Clear history")).on_hover_text(t("La quarantaine reste conservée.", "The quarantine is kept.")).clicked()
            && let Err(err) = app.history.clear() {
                log::warn!("historique non efface : {err:#}");
            }
    });

    let busy = app.is_busy();
    let mut undo = None;
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for session in &app.history.sessions {
            card(ui).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.label(RichText::new(&session.label).font(semibold(14.0)).color(p.text));
                        let text = format!(
                            "{} · {} {} · {}",
                            format_datetime(session.timestamp, lang),
                            format_count(session.operations.len() as u64, lang),
                            t("élément(s)", "item(s)"),
                            format_size(session.total_bytes(), lang),
                        );
                        ui.label(RichText::new(text).size(12.5).color(p.muted));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let restorable = session.restorable_count();
                        if restorable > 0 {
                            let label = format!("{}  {}", icons::ARROW_COUNTER_CLOCKWISE, t("Restaurer", "Restore"));
                            if button_enabled(ui, !busy, Kind::Secondary, &label).clicked() {
                                undo = Some(session.id.clone());
                            }
                        } else if session.operations.iter().all(|o| o.restored) {
                            badge(ui, t("Restauré", "Restored"), p.ok, p.ok_soft);
                        } else {
                            badge(ui, t("Non restaurable", "Not restorable"), p.muted, p.raised);
                        }
                    });
                });
                egui::CollapsingHeader::new(RichText::new(t("Détails", "Details")).size(12.5).color(p.muted))
                    .id_salt(&session.id)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        for op in session.operations.iter().take(200) {
                            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::hover());
                            let text = op.source.to_string_lossy();
                            paint_text(ui, ui.painter(), rect.left_center(), &text, body(12.5), p.muted, rect.width() - 110.0);
                            let (label, color) = if op.restored { (t("Restauré", "Restored"), p.ok) } else { (kind_label(op), p.faint) };
                            ui.painter().text(rect.right_center(), Align2::RIGHT_CENTER, label, body(12.0), color);
                        }
                        if session.operations.len() > 200 {
                            ui.label(RichText::new(format!("+{}", session.operations.len() - 200)).size(12.5).color(p.faint));
                        }
                    });
            });
        }
    });
    if let Some(id) = undo {
        app.actions.push(Action::Undo(id));
    }
}
