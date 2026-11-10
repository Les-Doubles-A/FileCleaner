//! Accueil : choix du dossier, avancement de l'analyse, tableau de bord.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, Layout, Pos2, Rect, RichText, Sense,
    Stroke, Ui, Vec2,
};
use filecleaner_core::config::category_label;
use filecleaner_core::duplicates::DupGroup;
use filecleaner_core::model::Scan;
use filecleaner_core::system::known_folders;
use filecleaner_core::util::{format_age, format_count, format_decimal, format_size};

use super::Page;
use crate::app::{Action, App};
use crate::icons;
use crate::jobs::{Derived, PickPurpose};
use crate::theme::{body, category_color, lang, pal, semibold, t};
use crate::widgets::{
    Kind, button, card, donut, empty_state, meter, paint_text, progress_bar, score_ring, section_title,
    sparkline, stat,
};

pub fn show(ui: &mut Ui, app: &mut App) {
    let scanning = app.scan.is_none() && app.is_busy();
    if scanning {
        scan_progress(ui, app);
    } else if let Some(scan) = app.scan.clone() {
        dashboard(ui, app, &scan);
    } else {
        welcome(ui, app);
    }
}

fn scan_progress(ui: &mut Ui, app: &mut App) {
    let Some(running) = &app.jobs.running else { return };
    let p = pal(ui);
    let lang = lang();
    let (files, _, bytes, _, current) = running.progress.snapshot();
    let progress = running.progress.clone();
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(80));
    let width = ui.available_width().min(520.0);
    let top = (ui.available_height() * 0.28).max(30.0);
    ui.vertical_centered(|ui| {
        ui.add_space(top);
        ui.set_max_width(width);
        ui.label(RichText::new(icons::MAGNIFYING_GLASS).size(40.0).color(p.accent));
        ui.label(RichText::new(t("Analyse en cours", "Scanning")).font(semibold(20.0)).color(p.text));
        ui.label(
            RichText::new(format!(
                "{} {} · {}",
                format_count(files, lang),
                t("fichiers", "files"),
                format_size(bytes, lang)
            ))
            .color(p.muted),
        );
        ui.add_space(8.0);
        progress_bar(ui, None, 6.0);
        ui.add_space(4.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 18.0), Sense::hover());
        paint_text(ui, ui.painter(), rect.left_center(), &current, body(12.0), p.faint, width);
        ui.add_space(8.0);
        if button(ui, Kind::Secondary, t("Annuler", "Cancel")).clicked() {
            progress.cancel();
        }
    });
}

fn location_tile(ui: &mut Ui, icon: &str, title: &str, subtitle: &str, width: f32, fill: Option<f32>) -> bool {
    let p = pal(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 66.0), Sense::click());
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    let stroke = if response.hovered() { p.accent } else { p.line };
    ui.painter().rect(rect, CornerRadius::same(10), p.card, Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
    let icon_center = rect.left_center() + Vec2::new(26.0, 0.0);
    ui.painter().text(icon_center, Align2::CENTER_CENTER, icon, body(22.0), p.accent);
    let x = rect.left() + 50.0;
    let text_width = rect.width() - 60.0;
    paint_text(ui, ui.painter(), Pos2::new(x, rect.top() + 22.0), title, semibold(14.0), p.text, text_width);
    paint_text(ui, ui.painter(), Pos2::new(x, rect.top() + 41.0), subtitle, body(12.0), p.muted, text_width);
    if let Some(ratio) = fill {
        let bar = Rect::from_min_size(Pos2::new(x, rect.bottom() - 12.0), Vec2::new(text_width, 3.0));
        let color = if ratio > 0.9 { p.danger } else { p.accent };
        meter(ui, ui.painter(), bar, ratio, color);
    }
    response.clicked()
}

fn welcome(ui: &mut Ui, app: &mut App) {
    let p = pal(ui);
    let lang = lang();
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        let width = ui.available_width().min(860.0);
        ui.vertical_centered(|ui| {
            ui.add_space(26.0);
            ui.set_max_width(width);
            ui.label(RichText::new(t("Que voulez-vous nettoyer ?", "What do you want to clean?")).font(semibold(26.0)).color(p.text));
            ui.label(
                RichText::new(t(
                    "Tout reste sur cet ordinateur. Rien n'est supprimé sans votre accord.",
                    "Everything stays on this computer. Nothing is deleted without your approval.",
                ))
                .color(p.muted),
            );
            ui.add_space(12.0);

            let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 150.0), Sense::click());
            let response = response.on_hover_cursor(CursorIcon::PointingHand);
            let color = if response.hovered() { p.accent } else { p.faint };
            let fill = if response.hovered() { p.accent_soft } else { p.card };
            ui.painter().rect_filled(rect, CornerRadius::same(14), fill);
            let dashes = egui::Shape::dashed_line(
                &[rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()]
                    .map(|c| c + (rect.center() - c).normalized() * 3.0),
                Stroke::new(1.2, color),
                7.0,
                6.0,
            );
            ui.painter().extend(dashes);
            let c = rect.center();
            ui.painter().text(c - Vec2::new(0.0, 28.0), Align2::CENTER_CENTER, icons::FOLDER_OPEN, body(36.0), p.accent);
            ui.painter().text(c + Vec2::new(0.0, 14.0), Align2::CENTER_CENTER, t("Glissez un dossier ici", "Drop a folder here"), semibold(16.0), p.text);
            ui.painter().text(c + Vec2::new(0.0, 36.0), Align2::CENTER_CENTER, t("ou cliquez pour parcourir (Ctrl O)", "or click to browse (Ctrl O)"), body(13.0), p.muted);
            if response.clicked() {
                app.actions.push(Action::Pick(PickPurpose::Scan));
            }
        });

        ui.add_space(14.0);
        let left_pad = ((ui.available_width() - width) / 2.0).max(0.0);
        let tile_w = ((width - 16.0) / 3.0).floor();
        let grid = |ui: &mut Ui, title: &str, tiles: Vec<(&str, String, String, Option<f32>, std::path::PathBuf)>, actions: &mut Vec<Action>| {
            if tiles.is_empty() {
                return;
            }
            ui.horizontal(|ui| {
                ui.add_space(left_pad);
                section_title(ui, title);
            });
            for row in tiles.chunks(3) {
                ui.horizontal(|ui| {
                    ui.add_space(left_pad);
                    for (icon, name, subtitle, fill, path) in row {
                        if location_tile(ui, icon, name, subtitle, tile_w, *fill) {
                            actions.push(Action::Scan(vec![path.clone()]));
                        }
                    }
                });
            }
            ui.add_space(8.0);
        };

        let folder_icon = |path: &std::path::Path| {
            let lower = path.to_string_lossy().to_lowercase();
            if lower.contains("download") || lower.contains("téléchargement") {
                icons::DOWNLOAD_SIMPLE
            } else if lower.contains("desktop") || lower.contains("bureau") {
                icons::DESKTOP
            } else if lower.contains("picture") || lower.contains("images") {
                icons::IMAGE
            } else if lower.contains("video") {
                icons::FILM_STRIP
            } else if lower.contains("music") || lower.contains("musique") {
                icons::MUSIC_NOTE
            } else {
                icons::FOLDER
            }
        };
        let known = known_folders()
            .into_iter()
            .map(|(label, path)| (folder_icon(&path), label.get(lang).to_string(), path.display().to_string(), None, path))
            .collect();
        grid(ui, t("Dossiers usuels", "Common folders"), known, &mut app.actions);

        let drives = app
            .drives
            .iter()
            .map(|d| {
                let used = d.total.saturating_sub(d.free);
                let subtitle = format!("{} {} {}", format_size(d.free, lang), t("libres sur", "free of"), format_size(d.total, lang));
                let name = if d.name.is_empty() { d.mount.display().to_string() } else { format!("{} ({})", d.name, d.mount.display()) };
                (icons::HARD_DRIVE, name, subtitle, Some(used as f32 / d.total.max(1) as f32), d.mount.clone())
            })
            .collect();
        grid(ui, t("Lecteurs", "Drives"), drives, &mut app.actions);

        let recents = app
            .settings
            .recent_folders
            .iter()
            .take(6)
            .map(|f| {
                let path = std::path::PathBuf::from(f);
                let name = path.file_name().map_or_else(|| f.clone(), |n| n.to_string_lossy().into_owned());
                (icons::CLOCK, name, f.clone(), None, path)
            })
            .collect();
        grid(ui, t("Récents", "Recent"), recents, &mut app.actions);
    });
}

fn opportunity(ui: &mut Ui, icon: &str, title: &str, detail: &str, value: &str, enabled: bool) -> bool {
    let p = pal(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 50.0), Sense::click());
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    if response.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(8), p.hover);
    }
    let color = if enabled { p.accent } else { p.faint };
    ui.painter().text(rect.left_center() + Vec2::new(20.0, 0.0), Align2::CENTER_CENTER, icon, body(20.0), color);
    let x = rect.left() + 42.0;
    let text_width = rect.width() - 170.0;
    paint_text(ui, ui.painter(), Pos2::new(x, rect.top() + 17.0), title, semibold(14.0), p.text, text_width);
    paint_text(ui, ui.painter(), Pos2::new(x, rect.top() + 34.0), detail, body(12.5), p.muted, text_width);
    let right = rect.right_center() - Vec2::new(26.0, 0.0);
    ui.painter().text(right, Align2::RIGHT_CENTER, value, semibold(14.0), if enabled { p.text } else { p.faint });
    ui.painter().text(rect.right_center() - Vec2::new(10.0, 0.0), Align2::CENTER_CENTER, icons::CARET_RIGHT, body(12.0), p.faint);
    response.clicked()
}

fn dashboard(ui: &mut Ui, app: &mut App, scan: &Scan) {
    let p = pal(ui);
    let lang = lang();
    let Some(derived) = app.derived.clone() else {
        empty_state(ui, icons::HOURGLASS, t("Préparation du tableau de bord", "Preparing the dashboard"), t("Calcul des statistiques...", "Computing statistics..."), None);
        return;
    };
    let dup_bytes: u64 = app.dup_groups.iter().map(DupGroup::recoverable).sum();
    let reclaim = derived.insights.junk_bytes + derived.insights.dev_bytes + dup_bytes;
    let score = derived.insights.health_score(scan.total_size(), dup_bytes);
    let snapshots = app.snapshots.history(scan).to_vec();

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.columns(4, |cols| {
            let delta = snapshots.len().checked_sub(2).map(|i| scan.total_size() as i64 - snapshots[i].size as i64);
            let hint = delta.filter(|d| *d != 0).map(|d| {
                let text = format!(
                    "{}{} {}",
                    if d > 0 { "+" } else { "-" },
                    format_size(d.unsigned_abs(), lang),
                    t("depuis la dernière analyse", "since last scan")
                );
                (text, if d > 0 { p.warn } else { p.ok })
            });
            stat(&mut cols[0], t("Espace analysé", "Scanned space"), &format_size(scan.total_size(), lang), hint.as_ref().map(|(s, c)| (s.as_str(), *c)));
            let duration = if scan.duration_ms < 1000 {
                format!("{} ms", scan.duration_ms.max(1))
            } else {
                format!("{} s", format_decimal(scan.duration_ms as f64 / 1000.0, 1, lang))
            };
            let in_time = format!("{} {duration}", t("analysés en", "scanned in"));
            stat(&mut cols[1], t("Fichiers", "Files"), &format_count(scan.total_files(), lang), Some((&in_time, p.muted)));
            let folders = format!("{} {}", t("analysé", "scanned"), format_age(scan.scanned_at, lang));
            stat(&mut cols[2], t("Dossiers", "Folders"), &format_count(scan.total_dirs(), lang), Some((&folders, p.muted)));
            let percent = format!("{} % {}", format_decimal(reclaim as f64 / scan.total_size().max(1) as f64 * 100.0, 1, lang), t("de l'espace", "of the space"));
            stat(&mut cols[3], t("Récupérable", "Recoverable"), &format_size(reclaim, lang), Some((&percent, p.ok)));
        });
        ui.add_space(4.0);

        ui.columns(2, |cols| {
            card(&cols[0]).show(&mut cols[0], |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    score_ring(ui, 92.0, score, t("propreté", "tidiness"));
                    ui.vertical(|ui| {
                        ui.add_space(8.0);
                        section_title(ui, t("Ce que vous pouvez faire", "What you can do"));
                        let summary = match score {
                            85.. => t("Ce dossier est bien tenu.", "This folder is in good shape."),
                            60.. => t("Quelques nettoyages rapides sont possibles.", "A few quick cleanups are possible."),
                            _ => t("Beaucoup d'espace peut être récupéré.", "A lot of space can be recovered."),
                        };
                        ui.label(RichText::new(summary).color(p.muted));
                        let label = format!("{}  {}", icons::LIGHTNING, t("Nettoyage express", "Express cleanup"));
                        let response = button(ui, Kind::Primary, &label).on_hover_text(t(
                            "Sélectionne uniquement les fichiers sans risque, puis demande confirmation.",
                            "Selects only risk-free files, then asks for confirmation.",
                        ));
                        if response.clicked() {
                            app.actions.push(Action::ExpressClean);
                        }
                    });
                });
                ui.add_space(4.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                opportunities(ui, app, &derived, dup_bytes);
            });

            card(&cols[1]).show(&mut cols[1], |ui| {
                ui.set_width(ui.available_width());
                section_title(ui, t("Répartition", "Breakdown"));
                ui.add_space(4.0);
                let total = scan.total_size().max(1);
                let parts: Vec<(f64, Color32)> = derived
                    .categories
                    .iter()
                    .map(|c| {
                        let index = scan.category_names.iter().position(|n| *n == c.name).unwrap_or(0);
                        (c.size as f64, category_color(index))
                    })
                    .collect();
                ui.horizontal_top(|ui| {
                    let hovered = ui.ctx().data(|d| d.get_temp::<usize>(egui::Id::unique("donut-hover")));
                    let (center, sub) = match hovered.and_then(|i| derived.categories.get(i)) {
                        Some(c) => (format_size(c.size, lang), category_label(&c.name, lang).to_string()),
                        None => (format_size(scan.total_size(), lang), t("au total", "in total").to_string()),
                    };
                    let now = donut(ui, 190.0, &parts, &center, &sub);
                    ui.ctx().data_mut(|d| {
                        if let Some(i) = now {
                            d.insert_temp(egui::Id::unique("donut-hover"), i);
                        } else {
                            d.remove_temp::<usize>(egui::Id::unique("donut-hover"));
                        }
                    });
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 5.0;
                        for (i, c) in derived.categories.iter().take(9).enumerate() {
                            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
                            let y = rect.center().y;
                            ui.painter().circle_filled(Pos2::new(rect.left() + 5.0, y), 4.5, parts[i].1);
                            paint_text(ui, ui.painter(), Pos2::new(rect.left() + 16.0, y), category_label(&c.name, lang), body(13.0), p.text, rect.width() - 110.0);
                            let text = format!("{:.0} %", c.size as f64 / total as f64 * 100.0);
                            ui.painter().text(Pos2::new(rect.right() - 62.0, y), Align2::RIGHT_CENTER, text, body(12.5), p.faint);
                            ui.painter().text(Pos2::new(rect.right(), y), Align2::RIGHT_CENTER, format_size(c.size, lang), body(12.5), p.muted);
                        }
                    });
                });
            });

            card(&cols[1]).show(&mut cols[1], |ui| {
                ui.set_width(ui.available_width());
                section_title(ui, t("Types qui prennent le plus de place", "Types taking the most space"));
                ui.spacing_mut().item_spacing.y = 2.0;
                let top = derived.extensions.first().map_or(1, |e| e.size).max(1);
                for e in derived.extensions.iter().take(7) {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
                    let y = rect.center().y;
                    let name = if e.ext.is_empty() { t("(sans extension)", "(no extension)").to_string() } else { format!(".{}", e.ext) };
                    paint_text(ui, ui.painter(), Pos2::new(rect.left(), y), &name, body(13.0), p.text, 110.0);
                    let bar = Rect::from_min_size(Pos2::new(rect.left() + 120.0, y - 3.0), Vec2::new(rect.width() - 300.0, 6.0));
                    meter(ui, ui.painter(), bar, e.size as f32 / top as f32, p.accent);
                    let count = format!("{} {}", format_count(e.count, lang), t("fich.", "files"));
                    ui.painter().text(Pos2::new(rect.right() - 76.0, y), Align2::RIGHT_CENTER, count, body(12.5), p.faint);
                    ui.painter().text(Pos2::new(rect.right(), y), Align2::RIGHT_CENTER, format_size(e.size, lang), body(13.0), p.muted);
                }
            });
            card(&cols[0]).show(&mut cols[0], |ui| {
                ui.set_width(ui.available_width());
                section_title(ui, t("Évolution de ce dossier", "This folder over time"));
                if snapshots.len() < 2 {
                    ui.label(RichText::new(t(
                        "Relancez une analyse plus tard pour suivre l'évolution de la taille.",
                        "Scan again later to follow how the size changes.",
                    ))
                    .color(p.muted));
                } else {
                    let values: Vec<f64> = snapshots.iter().map(|s| s.size as f64).collect();
                    let width = ui.available_width();
                    sparkline(ui, Vec2::new(width, 90.0), &values, p.accent);
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        let text = format!("{} {}", snapshots.len(), t("analyses enregistrées", "recorded scans"));
                        ui.label(RichText::new(text).size(12.5).color(p.faint));
                    });
                }
            });
        });
    });
}

fn opportunities(ui: &mut Ui, app: &mut App, derived: &Derived, dup_bytes: u64) {
    let lang = lang();
    let i = &derived.insights;
    let files = t("fichiers", "files");
    let dup_files: usize = app.dup_groups.iter().map(|g| g.files.len() - 1).sum();
    let dup_detail = if app.dup_done {
        format!("{} {}", format_count(dup_files as u64, lang), t("copies inutiles", "needless copies"))
    } else {
        t("Recherche en cours ou à lancer", "Running or not started").to_string()
    };
    let rows = [
        (Page::Duplicates, dup_detail, format_size(dup_bytes, lang), dup_bytes > 0),
        (Page::Junk, format!("{} {files}", format_count(i.junk_files, lang)), format_size(i.junk_bytes, lang), i.junk_bytes > 0),
        (Page::Dev, format!("{} {}", i.dev_dirs, t("dossiers régénérables", "rebuildable folders")), format_size(i.dev_bytes, lang), i.dev_bytes > 0),
        (Page::Big, format!("{} {files}", format_count(i.big_files, lang)), format_size(i.big_bytes, lang), i.big_files > 0),
        (Page::Old, format!("{} {files}", format_count(i.old_files, lang)), format_size(i.old_bytes, lang), i.old_files > 0),
        (Page::Empty, t("À retirer sans risque", "Safe to remove").to_string(), format_count(i.empty_dirs, lang), i.empty_dirs > 0),
    ];
    for (page, detail, value, enabled) in rows {
        if opportunity(ui, page.icon(), page.title(), &detail, &value, enabled) {
            app.actions.push(Action::Navigate(page));
        }
    }
}
