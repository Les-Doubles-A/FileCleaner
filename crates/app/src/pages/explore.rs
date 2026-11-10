//! Carte de l'espace disque : arborescence triee et treemap avec exploration.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, Margin, Pos2, Rect, RichText, Sense,
    Stroke, StrokeKind, Ui, Vec2,
};
use filecleaner_core::model::{DirId, FileId, Scan};
use filecleaner_core::treemap::{self, Rect as MapRect};
use filecleaner_core::util::{format_count, format_size};

use super::require_scan;
use crate::app::{Action, App};
use crate::icons;
use crate::table::file_icon;
use crate::theme::{body, category_color, lang, pal, semibold, t};
use crate::widgets::{card, meter, paint_check, paint_text};

const MAX_ITEMS: usize = 240;
const MAX_DEPTH: u8 = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TileKind {
    Dir(DirId),
    File(FileId),
    Rest(DirId, u32),
}

#[derive(Clone, Copy)]
struct Tile {
    rect: Rect,
    kind: TileKind,
    size: u64,
    depth: u8,
}

#[derive(Default)]
pub struct ExploreState {
    current: Option<DirId>,
    tiles: Vec<Tile>,
    key: Option<(DirId, u64, [u32; 2])>,
    menu_target: Option<TileKind>,
}

fn items_of(scan: &Scan, dir: DirId) -> Vec<(TileKind, u64)> {
    let node = scan.dir(dir);
    let mut items: Vec<(TileKind, u64)> = node
        .children
        .iter()
        .filter(|&&c| scan.dir(c).size > 0)
        .map(|&c| (TileKind::Dir(c), scan.dir(c).size))
        .collect();
    items.extend(
        node.files
            .clone()
            .filter(|&f| scan.file(f).is_alive() && scan.file(f).size > 0)
            .map(|f| (TileKind::File(f), scan.file(f).size)),
    );
    items.sort_by_key(|(_, size)| std::cmp::Reverse(*size));
    if items.len() > MAX_ITEMS {
        let rest: Vec<_> = items.split_off(MAX_ITEMS);
        let bytes = rest.iter().map(|(_, s)| *s).sum();
        items.push((TileKind::Rest(dir, rest.len() as u32), bytes));
        items.sort_by_key(|(_, size)| std::cmp::Reverse(*size));
    }
    items
}

fn layout(scan: &Scan, dir: DirId, bounds: Rect, depth: u8, out: &mut Vec<Tile>) {
    let items = items_of(scan, dir);
    let values: Vec<u64> = items.iter().map(|(_, s)| *s).collect();
    let map = MapRect { x: bounds.left(), y: bounds.top(), w: bounds.width(), h: bounds.height() };
    for ((kind, size), r) in items.into_iter().zip(treemap::squarify(&values, map)) {
        if r.w < 2.0 || r.h < 2.0 {
            continue;
        }
        let rect = Rect::from_min_size(Pos2::new(r.x, r.y), Vec2::new(r.w, r.h));
        out.push(Tile { rect, kind, size, depth });
        if let TileKind::Dir(child) = kind
            && depth < MAX_DEPTH && r.w > 64.0 && r.h > 48.0 {
                let inner = Rect::from_min_max(rect.min + Vec2::new(2.0, 19.0), rect.max - Vec2::splat(2.0));
                layout(scan, child, inner, depth + 1, out);
            }
    }
}

fn breadcrumb(ui: &mut Ui, scan: &Scan, current: DirId) -> Option<DirId> {
    let p = pal(ui);
    let mut chain = vec![current];
    let mut cursor = scan.dir(current).parent;
    while cursor != filecleaner_core::model::NO_DIR {
        chain.push(cursor);
        cursor = scan.dir(cursor).parent;
    }
    chain.reverse();
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (i, &dir) in chain.iter().enumerate() {
            let last = i + 1 == chain.len();
            let color = if last { p.text } else { p.muted };
            let name = if i == 0 { scan.dir(dir).path.display().to_string() } else { scan.dir(dir).name.to_string() };
            let response = ui.add(egui::Button::new(RichText::new(name).color(color)).frame(false));
            if response.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                clicked = Some(dir);
            }
            if !last {
                ui.label(RichText::new(icons::CARET_RIGHT).size(11.0).color(p.faint));
            }
        }
    });
    clicked
}

fn tile_label(scan: &Scan, tile: &Tile) -> String {
    match tile.kind {
        TileKind::Dir(d) => scan.dir(d).name.to_string(),
        TileKind::File(f) => scan.file(f).name.to_string(),
        TileKind::Rest(_, n) => format!("{n} {}", t("autres éléments", "other items")),
    }
}

fn paint_tile(ui: &Ui, scan: &Scan, tile: &Tile, hovered: bool) {
    let p = pal(ui);
    let painter = ui.painter();
    let radius = CornerRadius::same(if tile.depth == 0 { 6 } else { 3 });
    let rect = tile.rect.shrink(0.5);
    match tile.kind {
        TileKind::Dir(_) => {
            let shade = if p.dark { 26 + tile.depth * 8 } else { 240 - tile.depth * 10 };
            let fill = if p.dark {
                Color32::from_rgb(shade + 4, shade + 7, shade + 13)
            } else {
                Color32::from_rgb(shade - 6, shade - 3, shade)
            };
            painter.rect_filled(rect, radius, fill);
            if rect.height() > 22.0 && rect.width() > 40.0 {
                let label = tile_label(scan, tile);
                paint_text(ui, painter, Pos2::new(rect.left() + 6.0, rect.top() + 10.0), &label, semibold(12.0), p.text, rect.width() - 12.0);
            }
        }
        TileKind::File(id) => {
            let base = category_color(scan.file(id).category as usize);
            let fill = base.gamma_multiply(if p.dark { 0.72 } else { 0.85 });
            painter.rect_filled(rect, radius, fill);
            if rect.width() > 56.0 && rect.height() > 24.0 {
                let label = tile_label(scan, tile);
                paint_text(ui, painter, Pos2::new(rect.left() + 6.0, rect.top() + 12.0), &label, body(12.0), Color32::WHITE, rect.width() - 12.0);
                if rect.height() > 40.0 {
                    let size = format_size(tile.size, lang());
                    paint_text(ui, painter, Pos2::new(rect.left() + 6.0, rect.top() + 28.0), &size, body(11.5), Color32::from_white_alpha(200), rect.width() - 12.0);
                }
            }
        }
        TileKind::Rest(..) => {
            painter.rect_filled(rect, radius, p.line);
            if rect.width() > 70.0 && rect.height() > 22.0 {
                let label = tile_label(scan, tile);
                paint_text(ui, painter, Pos2::new(rect.left() + 6.0, rect.top() + 12.0), &label, body(12.0), p.muted, rect.width() - 12.0);
            }
        }
    }
    if hovered {
        painter.rect_stroke(rect, radius, Stroke::new(2.0, p.accent), StrokeKind::Inside);
    }
}

pub fn show(ui: &mut Ui, app: &mut App) {
    let Some(scan) = require_scan(ui, app) else { return };
    let p = pal(ui);
    let lang = lang();
    let state = &mut app.pages.explore;
    let current = match state.current.filter(|&d| !scan.dir(d).deleted) {
        Some(d) => d,
        None => {
            state.key = None;
            scan.root_dirs[0]
        }
    };
    state.current = Some(current);

    if scan.root_dirs.len() > 1 {
        ui.horizontal(|ui| {
            for &root in &scan.root_dirs {
                if crate::widgets::chip(ui, root == current, &scan.dir(root).name).clicked() {
                    state.current = Some(root);
                }
            }
        });
    }
    let parent = scan.dir(current).parent;
    ui.horizontal(|ui| {
        let can_go_up = parent != filecleaner_core::model::NO_DIR;
        if ui.add_enabled(can_go_up, egui::Button::new(RichText::new(icons::ARROW_UP).size(15.0))).on_hover_text(t("Dossier parent", "Parent folder")).clicked() {
            state.current = Some(parent);
        }
        if let Some(dir) = breadcrumb(ui, &scan, current) {
            state.current = Some(dir);
        }
    });

    let available = ui.available_size();
    let list_width = (available.x * 0.32).clamp(240.0, 380.0);
    ui.horizontal_top(|ui| {
        card(ui).inner_margin(Margin::same(8)).show(ui, |ui| {
            ui.set_width(list_width - 16.0);
            ui.set_height(available.y - 18.0);
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            let node = scan.dir(current);
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                ui.label(RichText::new(format_size(node.size, lang)).font(semibold(16.0)).color(p.text));
                ui.label(RichText::new(format!("{} {}", format_count(u64::from(node.file_count), lang), t("fichiers", "files"))).size(12.5).color(p.muted));
            });
            let items = items_of(&scan, current);
            let total = node.size.max(1);
            let row_h = 38.0;
            egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, row_h, items.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for &(kind, size) in &items[range] {
                    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::click());
                    if response.hovered() {
                        ui.painter().rect_filled(rect, CornerRadius::same(6), p.hover);
                    }
                    let (icon, color, label) = match kind {
                        TileKind::Dir(d) => (icons::FOLDER, p.accent, scan.dir(d).name.to_string()),
                        TileKind::File(f) => (file_icon(&scan, scan.file(f)), category_color(scan.file(f).category as usize), scan.file(f).name.to_string()),
                        TileKind::Rest(_, n) => (icons::DOTS_THREE, p.faint, format!("{n} {}", t("autres éléments", "other items"))),
                    };
                    let checked = match kind {
                        TileKind::Dir(d) => app.sel.dirs.contains(&d),
                        TileKind::File(f) => app.sel.files.contains(&f),
                        TileKind::Rest(..) => false,
                    };
                    let check_x = rect.left() + 14.0;
                    let on_check = response.hover_pos().is_some_and(|pos| pos.x < rect.left() + 28.0);
                    if !matches!(kind, TileKind::Rest(..)) {
                        paint_check(ui, ui.painter(), Pos2::new(check_x, rect.center().y), checked, false, on_check);
                    }
                    let y = rect.center().y - 5.0;
                    ui.painter().text(Pos2::new(rect.left() + 40.0, y), Align2::CENTER_CENTER, icon, body(15.0), color);
                    paint_text(ui, ui.painter(), Pos2::new(rect.left() + 54.0, y), &label, body(13.5), p.text, rect.width() - 130.0);
                    ui.painter().text(Pos2::new(rect.right() - 8.0, y), Align2::RIGHT_CENTER, format_size(size, lang), body(12.5), p.muted);
                    let bar = Rect::from_min_size(Pos2::new(rect.left() + 54.0, rect.bottom() - 9.0), Vec2::new(rect.width() - 62.0, 3.0));
                    meter(ui, ui.painter(), bar, size as f32 / total as f32, color);
                    if response.clicked() {
                        match kind {
                            TileKind::Dir(d) if on_check => app.sel.toggle_dir(&scan, d),
                            TileKind::File(f) if on_check => app.sel.toggle(&scan, f),
                            TileKind::Dir(d) => state.current = Some(d),
                            TileKind::File(f) => app.focus = Some(f),
                            TileKind::Rest(..) => {}
                        }
                    }
                    response.context_menu(|ui| tile_menu(ui, &scan, kind, &mut app.actions));
                }
            });
            });
        });

        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click());
        let size_key = [rect.width() as u32, rect.height() as u32];
        let key = (current, scan.generation, size_key);
        if state.key != Some(key) || state.current != Some(current) {
            state.tiles.clear();
            layout(&scan, current, rect, 0, &mut state.tiles);
            state.key = Some(key);
        }
        let hover_pos = response.hover_pos();
        let hovered = hover_pos.and_then(|pos| state.tiles.iter().rev().find(|tile| tile.rect.contains(pos)).copied());
        for tile in &state.tiles {
            let is_hover = hovered.is_some_and(|h| h.kind == tile.kind);
            paint_tile(ui, &scan, tile, is_hover);
        }
        if state.tiles.is_empty() {
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, t("Dossier vide", "Empty folder"), body(14.0), p.muted);
        }
        if let Some(tile) = hovered {
            let text = format!("{}\n{}", tile_label(&scan, &tile), format_size(tile.size, lang));
            let response = response.clone().on_hover_cursor(CursorIcon::PointingHand);
            response.show_tooltip_text(text);
        }
        if response.secondary_clicked() {
            state.menu_target = hovered.map(|tile| tile.kind);
        }
        if response.clicked() {
            match hovered.map(|tile| tile.kind) {
                Some(TileKind::Dir(d)) => state.current = Some(d),
                Some(TileKind::File(f)) => app.focus = Some(f),
                _ => {}
            }
        }
        let target = state.menu_target;
        response.context_menu(|ui| {
            if let Some(kind) = target {
                tile_menu(ui, &scan, kind, &mut app.actions);
                ui.separator();
                let label = format!("{}  {}", icons::CHECK_SQUARE, t("Basculer la sélection", "Toggle selection"));
                if ui.button(label).clicked() {
                    match kind {
                        TileKind::Dir(d) => app.sel.toggle_dir(&scan, d),
                        TileKind::File(f) => app.sel.toggle(&scan, f),
                        TileKind::Rest(..) => {}
                    }
                }
            }
        });
    });
}

fn tile_menu(ui: &mut Ui, scan: &Scan, kind: TileKind, actions: &mut Vec<Action>) {
    let path = match kind {
        TileKind::Dir(d) => scan.dir(d).path.clone(),
        TileKind::File(f) => scan.file_path(f),
        TileKind::Rest(d, _) => scan.dir(d).path.clone(),
    };
    if ui.button(format!("{}  {}", icons::FOLDER_OPEN, t("Afficher dans l'Explorateur", "Show in Explorer"))).clicked() {
        actions.push(Action::Reveal(path.clone()));
    }
    if ui.button(format!("{}  {}", icons::COPY, t("Copier le chemin", "Copy path"))).clicked() {
        ui.ctx().copy_text(path.display().to_string());
    }
}
