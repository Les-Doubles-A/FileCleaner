//! Composants visuels reutilisables.

use std::f32::consts::{FRAC_PI_2, TAU};

use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Margin, Pos2, Rect, Response,
    RichText, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, epaint::PathShape, text::LayoutJob,
    text::TextWrapping,
};

use crate::icons;
use crate::theme::{self, Palette, body, pal, semibold};

pub fn card(ui: &Ui) -> Frame {
    let p = pal(ui);
    Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.line))
        .corner_radius(CornerRadius::same(theme::RADIUS_LG))
        .inner_margin(Margin::same(16))
}

/// Texte peint, tronque a `width` avec points de suspension.
pub fn paint_text(
    ui: &Ui,
    painter: &egui::Painter,
    pos: Pos2,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
) -> f32 {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = TextWrapping::truncate_at_width(width.max(1.0));
    let galley = ui.painter().layout_job(job);
    let w = galley.size().x;
    let y = pos.y - galley.size().y / 2.0;
    painter.galley(Pos2::new(pos.x, y), galley, color);
    w
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

pub fn button(ui: &mut Ui, kind: Kind, text: &str) -> Response {
    let p = pal(ui);
    let (fill, color, stroke) = match kind {
        Kind::Primary => (p.accent, p.on_accent, Stroke::NONE),
        Kind::Secondary => (p.raised, p.text, Stroke::new(1.0, p.line)),
        Kind::Ghost => (Color32::TRANSPARENT, p.muted, Stroke::NONE),
        Kind::Danger => (p.danger, Color32::WHITE, Stroke::NONE),
    };
    ui.add(
        egui::Button::new(RichText::new(text).color(color))
            .fill(fill)
            .stroke(stroke)
            .corner_radius(CornerRadius::same(theme::RADIUS))
            .min_size(Vec2::new(0.0, 32.0)),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
}

pub fn button_enabled(ui: &mut Ui, enabled: bool, kind: Kind, text: &str) -> Response {
    ui.add_enabled_ui(enabled, |ui| button(ui, kind, text)).inner
}

pub fn icon_button(ui: &mut Ui, icon: &str, tip: &str) -> Response {
    let p = pal(ui);
    ui.add(
        egui::Button::new(RichText::new(icon).size(16.0).color(p.muted))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE)
            .min_size(Vec2::splat(32.0)),
    )
    .on_hover_text(tip)
    .on_hover_cursor(CursorIcon::PointingHand)
}

/// Pastille selectionnable (filtre, preset).
pub fn chip(ui: &mut Ui, selected: bool, text: &str) -> Response {
    let p = pal(ui);
    let (fill, color, stroke) = if selected {
        (p.accent_soft, p.text, Stroke::new(1.0, p.accent))
    } else {
        (Color32::TRANSPARENT, p.muted, Stroke::new(1.0, p.line))
    };
    ui.add(
        egui::Button::new(RichText::new(text).size(13.0).color(color))
            .fill(fill)
            .stroke(stroke)
            .corner_radius(CornerRadius::same(14))
            .min_size(Vec2::new(0.0, 28.0)),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
}

/// Etiquette non interactive.
pub fn badge(ui: &mut Ui, text: &str, fg: Color32, bg: Color32) {
    Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).color(fg));
        });
}

pub fn page_header(ui: &mut Ui, title: &str, subtitle: &str) {
    let p = pal(ui);
    ui.label(RichText::new(title).font(semibold(22.0)).color(p.text));
    if !subtitle.is_empty() {
        ui.add_space(-4.0);
        ui.label(RichText::new(subtitle).color(p.muted));
    }
    ui.add_space(6.0);
}

pub fn section_title(ui: &mut Ui, title: &str) {
    let p = pal(ui);
    ui.label(RichText::new(title).font(semibold(15.0)).color(p.text));
}

/// Etat vide centre : icone, message et action eventuelle.
pub fn empty_state(ui: &mut Ui, icon: &str, title: &str, message: &str, action: Option<&str>) -> bool {
    let p = pal(ui);
    let mut clicked = false;
    let height = ui.available_height();
    ui.vertical_centered(|ui| {
        ui.add_space((height * 0.28).max(24.0));
        ui.label(RichText::new(icon).size(44.0).color(p.faint));
        ui.add_space(6.0);
        ui.label(RichText::new(title).font(semibold(17.0)).color(p.text));
        ui.label(RichText::new(message).color(p.muted));
        if let Some(action) = action {
            ui.add_space(10.0);
            clicked = button(ui, Kind::Primary, action).clicked();
        }
    });
    clicked
}

/// Tuile de statistique.
pub fn stat(ui: &mut Ui, label: &str, value: &str, hint: Option<(&str, Color32)>) {
    let p = pal(ui);
    card(ui).show(ui, |ui| {
        ui.set_min_height(70.0);
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(RichText::new(label).size(13.0).color(p.muted));
        ui.label(RichText::new(value).font(semibold(24.0)).color(p.text));
        if let Some((hint, color)) = hint {
            ui.label(RichText::new(hint).size(12.5).color(color));
        }
    });
}

pub fn progress_bar(ui: &mut Ui, value: Option<f32>, height: f32) {
    let p = pal(ui);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let radius = CornerRadius::same((height / 2.0) as u8);
    ui.painter().rect_filled(rect, radius, p.line);
    match value {
        Some(v) => {
            let mut bar = rect;
            bar.set_width(rect.width() * v.clamp(0.0, 1.0));
            ui.painter().rect_filled(bar, radius, p.accent);
        }
        None => {
            // Indetermine : segment qui glisse.
            let time = ui.input(|i| i.time) as f32;
            let width = rect.width() * 0.25;
            let x = rect.left() + (time * 0.6).fract() * (rect.width() + width) - width;
            let bar = Rect::from_min_max(
                Pos2::new(x.max(rect.left()), rect.top()),
                Pos2::new((x + width).min(rect.right()), rect.bottom()),
            );
            ui.painter().rect_filled(bar, radius, p.accent);
            ui.ctx().request_repaint();
        }
    }
}

fn arc_points(center: Pos2, radius: f32, start: f32, end: f32) -> Vec<Pos2> {
    let steps = (((end - start).abs() / TAU) * 96.0).ceil().max(2.0) as usize;
    (0..=steps)
        .map(|i| {
            let a = start + (end - start) * i as f32 / steps as f32;
            center + radius * Vec2::angled(a)
        })
        .collect()
}

/// Anneau de repartition. Retourne l'index du segment survole.
pub fn donut(ui: &mut Ui, size: f32, parts: &[(f64, Color32)], center_text: &str, center_sub: &str) -> Option<usize> {
    let p = pal(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let center = rect.center();
    let thickness = size * 0.13;
    let radius = size / 2.0 - thickness / 2.0 - 2.0;
    let total: f64 = parts.iter().map(|(v, _)| *v).sum();
    let mut hovered = None;
    let hover_angle = response.hover_pos().and_then(|pos| {
        let d = pos - center;
        let dist = d.length();
        ((radius - thickness)..(radius + thickness)).contains(&dist).then(|| {
            let a = d.angle() + FRAC_PI_2;
            if a < 0.0 { a + TAU } else { a }
        })
    });
    ui.painter().circle_stroke(center, radius, Stroke::new(thickness, p.line));
    let mut start = 0.0_f32;
    if total > 0.0 {
        for (i, (value, color)) in parts.iter().enumerate() {
            let sweep = (*value / total) as f32 * TAU;
            if sweep < 0.002 {
                start += sweep;
                continue;
            }
            let is_hovered = hover_angle.is_some_and(|a| a >= start && a < start + sweep);
            if is_hovered {
                hovered = Some(i);
            }
            let gap = 0.012_f32.min(sweep * 0.2);
            let points = arc_points(center, radius, start + gap - FRAC_PI_2, start + sweep - gap - FRAC_PI_2);
            let width = if is_hovered { thickness + 4.0 } else { thickness };
            ui.painter().add(Shape::Path(PathShape::line(points, Stroke::new(width, *color))));
            start += sweep;
        }
    }
    ui.painter().text(center - Vec2::new(0.0, 9.0), Align2::CENTER_CENTER, center_text, semibold(size * 0.115), p.text);
    ui.painter().text(center + Vec2::new(0.0, 12.0), Align2::CENTER_CENTER, center_sub, body(12.5), p.muted);
    hovered
}

/// Jauge circulaire du score de proprete.
pub fn score_ring(ui: &mut Ui, size: f32, score: u8, label: &str) {
    let p = pal(ui);
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let center = rect.center();
    let thickness = size * 0.1;
    let radius = size / 2.0 - thickness / 2.0 - 1.0;
    let color = match score {
        75.. => p.ok,
        45.. => p.warn,
        _ => p.danger,
    };
    ui.painter().circle_stroke(center, radius, Stroke::new(thickness, p.line));
    let sweep = f32::from(score) / 100.0 * TAU;
    if sweep > 0.0 {
        let points = arc_points(center, radius, -FRAC_PI_2, sweep - FRAC_PI_2);
        ui.painter().add(Shape::Path(PathShape::line(points, Stroke::new(thickness, color))));
    }
    ui.painter().text(center - Vec2::new(0.0, 7.0), Align2::CENTER_CENTER, score, semibold(size * 0.26), p.text);
    ui.painter().text(center + Vec2::new(0.0, size * 0.17), Align2::CENTER_CENTER, label, body(12.0), p.muted);
}

/// Courbe d'evolution.
pub fn sparkline(ui: &mut Ui, size: Vec2, values: &[f64], color: Color32) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    if values.len() < 2 {
        return;
    }
    let (min, max) = values.iter().fold((f64::MAX, f64::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    let span = (max - min).max(1.0);
    let points: Vec<Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let x = rect.left() + rect.width() * i as f32 / (values.len() - 1) as f32;
            let y = rect.bottom() - 3.0 - ((v - min) / span) as f32 * (rect.height() - 6.0);
            Pos2::new(x, y)
        })
        .collect();
    let last = *points.last().unwrap_or(&rect.center());
    ui.painter().add(Shape::line(points, Stroke::new(1.8, color)));
    ui.painter().circle_filled(last, 3.0, color);
}

/// Barre horizontale proportionnelle.
pub fn meter(ui: &Ui, painter: &egui::Painter, rect: Rect, ratio: f32, color: Color32) {
    let p = pal(ui);
    painter.rect_filled(rect, CornerRadius::same(2), p.line);
    let mut bar = rect;
    bar.set_width((rect.width() * ratio.clamp(0.0, 1.0)).max(2.0));
    painter.rect_filled(bar, CornerRadius::same(2), color);
}

/// Case a cocher peinte, sans allocation de layout.
pub fn paint_check(ui: &Ui, painter: &egui::Painter, center: Pos2, checked: bool, mixed: bool, hovered: bool) {
    let p = pal(ui);
    let rect = Rect::from_center_size(center, Vec2::splat(16.0));
    let radius = CornerRadius::same(4);
    if checked || mixed {
        painter.rect_filled(rect, radius, p.accent);
        let glyph = if checked { icons::CHECK } else { icons::MINUS };
        painter.text(center, Align2::CENTER_CENTER, glyph, semibold(12.0), p.on_accent);
    } else {
        let stroke = if hovered { p.muted } else { p.faint };
        painter.rect_stroke(rect, radius, Stroke::new(1.2, stroke), StrokeKind::Inside);
    }
}

/// Bloc d'avertissement ou d'information.
pub fn notice(ui: &mut Ui, icon: &str, text: &str, fg: Color32, bg: Color32) {
    Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(icon).color(fg).size(16.0));
                ui.label(RichText::new(text).color(fg));
            });
        });
}

/// Champ de recherche avec icone. Retourne la reponse du champ.
pub fn search_box(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    let p = pal(ui);
    let mut result = None;
    Frame::new()
        .fill(p.raised)
        .stroke(Stroke::new(1.0, p.line))
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(Margin::symmetric(10, 0))
        .show(ui, |ui| {
            ui.set_width(width);
            ui.set_height(32.0);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(RichText::new(icons::MAGNIFYING_GLASS).color(p.faint).size(15.0));
                let clear = !text.is_empty();
                let reserve = if clear { 28.0 } else { 0.0 };
                let response = ui.add(
                    egui::TextEdit::singleline(text)
                        .hint_text(hint.to_string())
                        .frame(Frame::NONE)
                        .align(egui::Align2::LEFT_CENTER)
                        .desired_width(ui.available_width() - reserve),
                );
                if clear && ui.add(egui::Button::new(RichText::new(icons::X).size(12.0).color(p.faint)).frame(false)).clicked() {
                    text.clear();
                }
                result = Some(response);
            });
        });
    result.expect("champ de recherche")
}

pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let p = pal(ui);
    let mut changed = false;
    Frame::new()
        .fill(p.raised)
        .stroke(Stroke::new(1.0, p.line))
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(Margin::same(2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (option, label) in options {
                    let selected = *value == *option;
                    let (fill, color) = if selected { (p.accent, p.on_accent) } else { (Color32::TRANSPARENT, p.muted) };
                    let response = ui.add(
                        egui::Button::new(RichText::new(*label).size(13.0).color(color))
                            .fill(fill)
                            .stroke(Stroke::NONE)
                            .corner_radius(CornerRadius::same(6))
                            .min_size(Vec2::new(0.0, 26.0)),
                    );
                    if response.clicked() && !selected {
                        *value = *option;
                        changed = true;
                    }
                }
            });
        });
    changed
}

/// Interrupteur avec libelle.
pub fn toggle(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let p: Palette = pal(ui);
    let size = Vec2::new(36.0, 20.0);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), body(14.0), p.text);
    let total = Vec2::new(size.x + 10.0 + galley.size().x, 24.0);
    let (rect, mut response) = ui.allocate_exact_size(total, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let how_on = ui.ctx().animate_bool_responsive(response.id, *on);
    let track = Rect::from_min_size(Pos2::new(rect.left(), rect.center().y - size.y / 2.0), size);
    let fill = if *on { p.accent } else { p.line };
    ui.painter().rect_filled(track, CornerRadius::same(10), fill);
    let x = egui::lerp(track.left() + 10.0..=track.right() - 10.0, how_on);
    ui.painter().circle_filled(Pos2::new(x, track.center().y), 7.0, if *on { p.on_accent } else { p.muted });
    ui.painter().galley(
        Pos2::new(track.right() + 10.0, rect.center().y - galley.size().y / 2.0),
        galley,
        p.text,
    );
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// Entier borne avec libelle, valeur saisissable au clavier ou a la souris.
pub fn number(ui: &mut Ui, value: &mut u64, range: std::ops::RangeInclusive<u64>, suffix: &str) -> Response {
    ui.add(egui::DragValue::new(value).range(range).suffix(suffix.to_string()).speed(1.0))
}
