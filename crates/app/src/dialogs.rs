//! Couches superposees : revue avant suppression, palette de commandes, notifications.

use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Frame, Id, Key, Layout, Margin, Modal, Modifiers,
    Order, RichText, Sense, Stroke, Vec2,
};
use filecleaner_core::actions::{DeleteMode, Target};
use filecleaner_core::duplicates::DupMode;
use filecleaner_core::safety::protection_reason;
use filecleaner_core::util::{format_count, format_size};

use crate::app::{Action, App};
use crate::icons;
use crate::jobs::{ExportFormat, PickPurpose};
use crate::pages::Page;
use crate::theme::{body, lang, palette, semibold, t};
use crate::widgets::{Kind, button, button_enabled, notice, paint_text, segmented, toggle};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Info,
    Error,
}

pub struct Toast {
    kind: ToastKind,
    text: String,
    undo_session: Option<String>,
    expires: Instant,
}

impl Toast {
    pub fn offers_undo_of(&self, session: &str) -> bool {
        self.undo_session.as_deref() == Some(session)
    }

    pub fn new(kind: ToastKind, text: String, undo_session: Option<String>) -> Self {
        let seconds = if undo_session.is_some() || kind == ToastKind::Error { 10 } else { 5 };
        Self { kind, text, undo_session, expires: Instant::now() + Duration::from_secs(seconds) }
    }
}

pub fn show_toasts(ctx: &egui::Context, app: &mut App) {
    let now = Instant::now();
    app.toasts.retain(|toast| toast.expires > now);
    if app.toasts.is_empty() {
        return;
    }
    ctx.request_repaint_after(Duration::from_millis(250));
    let p = palette(ctx.global_style().visuals.dark_mode);
    let bottom = if app.sel.is_empty() { -16.0 } else { -76.0 };
    let mut dismissed = None;
    let mut undo = None;
    egui::Area::new(Id::unique("toasts"))
        .order(Order::Foreground)
        .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-16.0, bottom))
        .show(ctx, |ui| {
            ui.set_max_width(420.0);
            for (index, toast) in app.toasts.iter().enumerate().rev().take(4) {
                let (icon, color) = match toast.kind {
                    ToastKind::Success => (icons::CHECK_CIRCLE, p.ok),
                    ToastKind::Info => (icons::INFO, p.accent),
                    ToastKind::Error => (icons::WARNING_CIRCLE, p.danger),
                };
                Frame::new()
                    .fill(p.raised)
                    .stroke(Stroke::new(1.0, p.line))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::symmetric(12, 10))
                    .shadow(ui.visuals().popup_shadow)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(icon).size(18.0).color(color));
                            ui.add(egui::Label::new(RichText::new(&toast.text).color(p.text)).wrap());
                            if let Some(session) = &toast.undo_session
                                && ui.add(egui::Button::new(RichText::new(t("Annuler", "Undo")).color(p.accent)).frame(false)).clicked() {
                                    undo = Some((index, session.clone()));
                                }
                            if ui.add(egui::Button::new(RichText::new(icons::X).size(12.0).color(p.faint)).frame(false)).clicked() {
                                dismissed = Some(index);
                            }
                        });
                    });
            }
        });
    if let Some((index, session)) = undo {
        app.toasts.remove(index);
        app.actions.push(Action::Undo(session));
    } else if let Some(index) = dismissed {
        app.toasts.remove(index);
    }
}

pub fn show_drop_overlay(ctx: &egui::Context) {
    if ctx.input(|i| i.raw.hovered_files.is_empty()) {
        return;
    }
    let p = palette(ctx.global_style().visuals.dark_mode);
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(Order::Foreground, Id::unique("drop")));
    painter.rect_filled(screen, 0, Color32::from_black_alpha(170));
    painter.rect_stroke(screen.shrink(24.0), 16, Stroke::new(2.0, p.accent), egui::StrokeKind::Inside);
    painter.text(screen.center() - Vec2::new(0.0, 22.0), Align2::CENTER_CENTER, icons::FOLDER_OPEN, body(44.0), p.accent);
    painter.text(
        screen.center() + Vec2::new(0.0, 22.0),
        Align2::CENTER_CENTER,
        t("Déposez pour analyser", "Drop to scan"),
        semibold(18.0),
        Color32::WHITE,
    );
}

/// Derniere etape avant toute suppression.
pub struct Review {
    pub title: String,
    pub targets: Vec<Target>,
    pub mode: DeleteMode,
    pub understood: bool,
    pub warnings: Vec<String>,
    total: u64,
    protected: usize,
}

impl Review {
    pub fn new(title: String, targets: Vec<Target>, mode: DeleteMode, warnings: Vec<String>) -> Self {
        let total = targets.iter().map(|target| target.size).sum();
        let protected = targets.iter().filter(|target| protection_reason(&target.path).is_some()).count();
        Self { title, targets, mode, understood: false, warnings, total, protected }
    }
}

pub fn show_review(ctx: &egui::Context, app: &mut App) {
    let Some(review) = &mut app.review else { return };
    let p = palette(ctx.global_style().visuals.dark_mode);
    let lang = lang();
    let mut confirmed = false;
    let mut cancelled = false;
    let response = Modal::new(Id::unique("review"))
        .frame(
            Frame::new()
                .fill(p.card)
                .stroke(Stroke::new(1.0, p.line))
                .corner_radius(CornerRadius::same(14))
                .inner_margin(Margin::same(22)),
        )
        .show(ctx, |ui| {
            ui.set_width(560.0);
            ui.label(RichText::new(&review.title).font(semibold(19.0)).color(p.text));
            ui.label(
                RichText::new(format!(
                    "{} {} · {}",
                    format_count(review.targets.len() as u64, lang),
                    t("élément(s)", "item(s)"),
                    format_size(review.total, lang),
                ))
                .color(p.muted),
            );
            ui.add_space(6.0);

            Frame::new().fill(p.raised).corner_radius(CornerRadius::same(8)).inner_margin(Margin::symmetric(4, 4)).show(ui, |ui| {
                let height = 22.0;
                egui::ScrollArea::vertical().max_height(190.0).auto_shrink([false, true]).show_rows(
                    ui,
                    height,
                    review.targets.len(),
                    |ui, range| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for target in &review.targets[range] {
                            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
                            let icon = if target.is_dir { icons::FOLDER } else { icons::FILE };
                            ui.painter().text(rect.left_center() + Vec2::new(12.0, 0.0), Align2::CENTER_CENTER, icon, body(13.0), p.faint);
                            let name = target.path.file_name().map_or_else(|| target.path.to_string_lossy(), |n| n.to_string_lossy());
                            let used = paint_text(ui, ui.painter(), rect.left_center() + Vec2::new(26.0, 0.0), &name, body(12.5), p.text, (rect.width() - 110.0) * 0.6);
                            if let Some(parent) = target.path.parent() {
                                let folder = parent.to_string_lossy();
                                let x = 34.0 + used;
                                paint_text(ui, ui.painter(), rect.left_center() + Vec2::new(x, 0.0), &folder, body(12.0), p.faint, rect.width() - 90.0 - x);
                            }
                            ui.painter().text(rect.right_center() - Vec2::new(8.0, 0.0), Align2::RIGHT_CENTER, format_size(target.size, lang), body(12.5), p.text);
                        }
                    },
                );
            });
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.label(RichText::new(t("Destination", "Destination")).color(p.muted));
                segmented(ui, &mut review.mode, &[
                    (DeleteMode::Trash, DeleteMode::Trash.label().get(lang)),
                    (DeleteMode::Quarantine, DeleteMode::Quarantine.label().get(lang)),
                    (DeleteMode::Permanent, DeleteMode::Permanent.label().get(lang)),
                ]);
            });
            let explain = match review.mode {
                DeleteMode::Trash => t("Récupérable depuis la Corbeille ou l'historique.", "Recoverable from the Recycle Bin or history."),
                DeleteMode::Quarantine => t("Déplacé dans un dossier caché, restaurable à tout moment.", "Moved to a hidden folder, restorable at any time."),
                DeleteMode::Permanent => t("Espace libéré immédiatement, aucune restauration possible.", "Space freed at once, no way back."),
            };
            ui.label(RichText::new(explain).size(13.0).color(p.muted));

            for warning in &review.warnings {
                notice(ui, icons::WARNING, warning, p.warn, p.warn_soft);
            }
            if review.protected > 0 {
                let text = format!("{} {}", review.protected, t("élément(s) protégé(s) par le système seront ignorés.", "system-protected item(s) will be skipped."));
                notice(ui, icons::SHIELD_WARNING, &text, p.warn, p.warn_soft);
            }
            if review.mode == DeleteMode::Permanent {
                notice(ui, icons::WARNING, t("Cette action est irréversible.", "This action cannot be undone."), p.danger, p.danger_soft);
                toggle(ui, &mut review.understood, t("Je comprends, supprimer définitivement", "I understand, delete permanently"));
            }
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let n = format_count(review.targets.len() as u64, lang);
                let (kind, label, enabled) = match review.mode {
                    DeleteMode::Trash => (Kind::Primary, format!("{} {n} {}", t("Envoyer", "Send"), t("à la Corbeille", "to the Recycle Bin")), true),
                    DeleteMode::Quarantine => (Kind::Primary, format!("{} {n} {}", t("Mettre", "Put"), t("en quarantaine", "in quarantine")), true),
                    DeleteMode::Permanent => (Kind::Danger, format!("{} {n}", t("Supprimer définitivement", "Permanently delete")), review.understood),
                };
                if button_enabled(ui, enabled, kind, &label).clicked() {
                    confirmed = true;
                }
                if button(ui, Kind::Secondary, t("Annuler", "Cancel")).clicked() {
                    cancelled = true;
                }
            });
        });
    if confirmed {
        if let Some(review) = app.review.take() {
            app.run_delete(review.title, review.targets, review.mode);
        }
    } else if cancelled || response.should_close() {
        app.review = None;
    }
}

struct Command {
    icon: &'static str,
    label: String,
    keywords: &'static str,
    shortcut: &'static str,
    run: Run,
}

enum Run {
    Do(fn() -> Action),
    Page(Page),
    Recent(String),
}

fn commands(app: &App) -> Vec<Command> {
    let mut out: Vec<Command> = vec![
        Command { icon: icons::FOLDER_OPEN, label: t("Analyser un dossier", "Scan a folder").into(), keywords: "ouvrir open scan analyser", shortcut: "Ctrl O", run: Run::Do(|| Action::Pick(PickPurpose::Scan)) },
        Command { icon: icons::ARROWS_CLOCKWISE, label: t("Relancer l'analyse", "Scan again").into(), keywords: "rescan refresh actualiser", shortcut: "F5", run: Run::Do(|| Action::Rescan) },
        Command { icon: icons::ARROW_COUNTER_CLOCKWISE, label: t("Annuler la dernière action", "Undo last action").into(), keywords: "undo annuler restaurer", shortcut: "Ctrl Z", run: Run::Do(|| Action::UndoLast) },
        Command { icon: icons::LIGHTNING, label: t("Nettoyage express", "Express cleanup").into(), keywords: "rapide quick sur safe", shortcut: "", run: Run::Do(|| Action::ExpressClean) },
        Command { icon: icons::COPY, label: t("Chercher les doublons", "Find duplicates").into(), keywords: "duplicates hash", shortcut: "", run: Run::Do(|| Action::FindDuplicates(DupMode::Content)) },
        Command { icon: icons::IMAGES, label: t("Chercher les images similaires", "Find similar images").into(), keywords: "photos similar", shortcut: "", run: Run::Do(|| Action::FindSimilar) },
        Command { icon: icons::EXPORT, label: t("Exporter un rapport HTML", "Export an HTML report").into(), keywords: "export html", shortcut: "", run: Run::Do(|| Action::Export(ExportFormat::Html)) },
        Command { icon: icons::EXPORT, label: t("Exporter un rapport PDF", "Export a PDF report").into(), keywords: "export pdf", shortcut: "", run: Run::Do(|| Action::Export(ExportFormat::Pdf)) },
    ];
    let settings = [Page::Settings];
    for pages in Page::groups().iter().map(|(_, pages)| *pages).chain([&settings[..]]) {
        for &page in pages {
            out.push(Command {
                icon: page.icon(),
                label: format!("{} {}", t("Aller à :", "Go to:"), page.title()),
                keywords: "page aller",
                shortcut: "",
                run: Run::Page(page),
            });
        }
    }
    for folder in &app.settings.recent_folders {
        out.push(Command {
            icon: icons::CLOCK,
            label: folder.clone(),
            keywords: "recent",
            shortcut: "",
            run: Run::Recent(folder.clone()),
        });
    }
    out
}

#[derive(Default)]
pub struct Palette {
    query: String,
    cursor: usize,
    focused: bool,
}

pub fn show_palette(ctx: &egui::Context, app: &mut App) {
    if app.command_palette.is_none() {
        return;
    }
    let all = commands(app);
    let Some(palette_state) = &mut app.command_palette else { return };
    let p = palette(ctx.global_style().visuals.dark_mode);
    let needle = palette_state.query.to_lowercase();
    let list: Vec<&Command> = all
        .iter()
        .filter(|c| needle.split_whitespace().all(|w| c.label.to_lowercase().contains(w) || c.keywords.contains(w)))
        .collect();
    palette_state.cursor = palette_state.cursor.min(list.len().saturating_sub(1));

    let (up, down, enter, escape) = ctx.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::Enter),
            i.consume_key(Modifiers::NONE, Key::Escape),
        )
    });
    if up {
        palette_state.cursor = palette_state.cursor.saturating_sub(1);
    }
    if down {
        palette_state.cursor = (palette_state.cursor + 1).min(list.len().saturating_sub(1));
    }
    let mut chosen = enter.then_some(palette_state.cursor);

    let width = 560.0;
    let screen = ctx.content_rect();
    let area = egui::Area::new(Id::unique("palette-area"))
        .kind(egui::UiKind::Modal)
        .sense(Sense::hover())
        .order(Order::Foreground)
        .interactable(true)
        .fixed_pos(egui::pos2(screen.center().x - width / 2.0, screen.top() + 90.0));
    let response = Modal::new(Id::unique("palette"))
        .area(area)
        .frame(
            Frame::new()
                .fill(p.card)
                .stroke(Stroke::new(1.0, p.line))
                .corner_radius(CornerRadius::same(14))
                .inner_margin(Margin::same(10)),
        )
        .show(ctx, |ui| {
            ui.set_width(width);
            let edit = ui.add(
                egui::TextEdit::singleline(&mut palette_state.query)
                    .hint_text(t("Rechercher une commande, une page, un dossier récent", "Search a command, a page, a recent folder"))
                    .frame(Frame::NONE)
                    .margin(Margin::symmetric(8, 8))
                    .font(body(16.0))
                    .desired_width(f32::INFINITY),
            );
            if !palette_state.focused {
                edit.request_focus();
                palette_state.focused = true;
            }
            ui.separator();
            egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                if list.is_empty() {
                    ui.label(RichText::new(t("Aucun résultat", "No results")).color(p.muted));
                }
                for (index, command) in list.iter().enumerate() {
                    let (rect, row) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::click());
                    let active = index == palette_state.cursor;
                    if active || row.hovered() {
                        ui.painter().rect_filled(rect, CornerRadius::same(8), if active { p.accent_soft } else { p.hover });
                    }
                    if active && (up || down) {
                        ui.scroll_to_rect(rect, None);
                    }
                    ui.painter().text(rect.left_center() + Vec2::new(18.0, 0.0), Align2::CENTER_CENTER, command.icon, body(16.0), p.muted);
                    paint_text(ui, ui.painter(), rect.left_center() + Vec2::new(38.0, 0.0), &command.label, body(14.0), p.text, rect.width() - 110.0);
                    ui.painter().text(rect.right_center() - Vec2::new(10.0, 0.0), Align2::RIGHT_CENTER, command.shortcut, body(12.0), p.faint);
                    if row.clicked() {
                        chosen = Some(index);
                    }
                }
            });
        });

    let close = escape || response.should_close();
    let selected = chosen.and_then(|index| list.get(index)).map(|command| match &command.run {
        Run::Do(make) => make(),
        Run::Page(page) => Action::Navigate(*page),
        Run::Recent(folder) => Action::Scan(vec![folder.into()]),
    });
    if let Some(action) = selected {
        app.actions.push(action);
        app.command_palette = None;
    } else if close {
        app.command_palette = None;
    }
}
