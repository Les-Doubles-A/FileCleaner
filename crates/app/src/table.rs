//! Table de fichiers virtualisee : seules les lignes visibles sont dessinees,
//! ce qui reste fluide avec des millions d'entrees.

use std::collections::HashSet;

use eframe::egui::{self, Align2, CornerRadius, Key, Modifiers, Pos2, Rect, Sense, Ui, Vec2};
use filecleaner_core::model::{DirId, FileEntry, FileId, Scan, flags};
use filecleaner_core::search::Query;
use filecleaner_core::util::{format_date, format_size};

use crate::app::Action;
use crate::icons;
use crate::theme::{body, category_color, lang, pal, semibold, t};
use crate::widgets::{meter, paint_check, paint_text};

/// Selection globale, partagee par toutes les pages.
#[derive(Default)]
pub struct Selection {
    pub files: HashSet<FileId>,
    pub dirs: HashSet<DirId>,
    pub bytes: u64,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.dirs.is_empty()
    }
    pub fn count(&self) -> usize {
        self.files.len() + self.dirs.len()
    }
    pub fn clear(&mut self) {
        self.files.clear();
        self.dirs.clear();
        self.bytes = 0;
    }
    pub fn toggle(&mut self, scan: &Scan, id: FileId) {
        if self.files.remove(&id) {
            self.bytes = self.bytes.saturating_sub(scan.file(id).size);
        } else {
            self.files.insert(id);
            self.bytes += scan.file(id).size;
        }
    }
    pub fn set(&mut self, scan: &Scan, id: FileId, on: bool) {
        if on != self.files.contains(&id) {
            self.toggle(scan, id);
        }
    }
    pub fn toggle_dir(&mut self, scan: &Scan, id: DirId) {
        if self.dirs.remove(&id) {
            self.bytes = self.bytes.saturating_sub(scan.dir(id).size);
        } else {
            self.dirs.insert(id);
            self.bytes += scan.dir(id).size;
        }
    }
    /// Retire les entrees disparues apres une action.
    pub fn prune(&mut self, scan: &Scan) {
        self.files.retain(|&f| scan.file(f).is_alive());
        self.dirs.retain(|&d| !scan.dir(d).deleted);
        self.bytes = self.files.iter().map(|&f| scan.file(f).size).sum::<u64>()
            + self.dirs.iter().map(|&d| scan.dir(d).size).sum::<u64>();
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SortKey {
    Name,
    Folder,
    #[default]
    Size,
    Modified,
    Tag,
}

/// Etat conserve par table : tri, filtre, ordre calcule.
pub struct TableState {
    pub sort: SortKey,
    pub descending: bool,
    pub filter: String,
    order: Vec<FileId>,
    key: Option<(u64, usize, usize, SortKey, bool, String)>,
    anchor: Option<usize>,
    cursor: Option<usize>,
    scroll_to_cursor: bool,
}

impl Default for TableState {
    fn default() -> Self {
        Self {
            sort: SortKey::Size,
            descending: true,
            filter: String::new(),
            order: Vec::new(),
            key: None,
            anchor: None,
            cursor: None,
            scroll_to_cursor: false,
        }
    }
}

impl TableState {
    pub fn sorted_by(sort: SortKey, descending: bool) -> Self {
        Self { sort, descending, ..Self::default() }
    }

    fn refresh(&mut self, scan: &Scan, rows: &[FileId], tag: Option<&dyn Fn(FileId) -> String>) {
        let key = (scan.generation, rows.as_ptr() as usize, rows.len(), self.sort, self.descending, self.filter.clone());
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.key = Some(key);
        let query = Query::parse(&self.filter);
        self.order = rows
            .iter()
            .copied()
            .filter(|&id| scan.file(id).is_alive() && (query.is_empty() || query.matches(scan, id)))
            .collect();
        let by = |id: FileId| scan.file(id);
        match self.sort {
            SortKey::Name => self.order.sort_by_cached_key(|&id| by(id).name.to_lowercase()),
            SortKey::Folder => self.order.sort_by_cached_key(|&id| scan.folder_of(id).to_path_buf()),
            SortKey::Size => self.order.sort_by_key(|&id| by(id).size),
            SortKey::Modified => self.order.sort_by_key(|&id| by(id).modified),
            SortKey::Tag => {
                if let Some(tag) = tag {
                    self.order.sort_by_cached_key(|&id| tag(id));
                }
            }
        }
        if self.descending {
            self.order.reverse();
        }
        self.cursor = None;
        self.anchor = None;
    }
}

pub struct Env<'a> {
    pub scan: &'a Scan,
    pub sel: &'a mut Selection,
    pub focus: &'a mut Option<FileId>,
    pub actions: &'a mut Vec<Action>,
    pub row_height: f32,
}

/// Chemin relatif a la racine quand l'analyse n'en a qu'une.
pub fn short_folder(scan: &Scan, path: &std::path::Path) -> String {
    match (scan.roots.as_slice(), scan.roots.first().and_then(|root| path.strip_prefix(root).ok())) {
        ([_], Some(relative)) if relative.as_os_str().is_empty() => ".".to_string(),
        ([_], Some(relative)) => relative.display().to_string(),
        _ => path.display().to_string(),
    }
}

pub fn file_icon(scan: &Scan, file: &FileEntry) -> &'static str {
    match scan.category_of(file) {
        "Images" => icons::IMAGE,
        "Vidéos" => icons::FILM_STRIP,
        "Audio" => icons::MUSIC_NOTE,
        "PDF" => icons::FILE_PDF,
        "Documents" => icons::FILE_TEXT,
        "Archives" => icons::FILE_ZIP,
        "Code" => icons::FILE_CODE,
        "Applications" => icons::APP_WINDOW,
        "Bases de données" => icons::DATABASE,
        "Polices" => icons::TEXT_AA,
        "E-books" => icons::BOOK_OPEN,
        "Modèles 3D" => icons::CUBE,
        _ => icons::FILE,
    }
}

/// Menu contextuel commun aux fichiers.
pub fn file_menu(ui: &mut Ui, env: &mut Env<'_>, id: FileId) {
    let path = env.scan.file_path(id);
    let folder = env.scan.folder_of(id).to_path_buf();
    if ui.button(format!("{}  {}", icons::ARROW_SQUARE_OUT, t("Ouvrir", "Open"))).clicked() {
        env.actions.push(Action::OpenPath(path.clone()));
    }
    if ui.button(format!("{}  {}", icons::FOLDER_OPEN, t("Afficher dans l'Explorateur", "Show in Explorer"))).clicked() {
        env.actions.push(Action::Reveal(path.clone()));
    }
    if ui.button(format!("{}  {}", icons::COPY, t("Copier le chemin", "Copy path"))).clicked() {
        ui.ctx().copy_text(path.display().to_string());
    }
    ui.separator();
    let selected = env.sel.files.contains(&id);
    let label = if selected { t("Retirer de la sélection", "Remove from selection") } else { t("Ajouter à la sélection", "Add to selection") };
    if ui.button(format!("{}  {label}", icons::CHECK_SQUARE)).clicked() {
        env.sel.toggle(env.scan, id);
    }
    if ui.button(format!("{}  {}", icons::EYE_SLASH, t("Exclure ce dossier des analyses", "Exclude this folder from scans"))).clicked() {
        env.actions.push(Action::ExcludeFolder(folder));
    }
}

fn sort_header(ui: &mut Ui, state: &mut TableState, rect: Rect, key: SortKey, label: &str, right: bool) {
    let p = pal(ui);
    let id = ui.scope_id().with(("sort", label));
    let response = ui.interact(rect, id, Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.clicked() {
        if state.sort == key {
            state.descending = !state.descending;
        } else {
            state.sort = key;
            state.descending = matches!(key, SortKey::Size | SortKey::Modified);
        }
    }
    let active = state.sort == key;
    let color = if active || response.hovered() { p.text } else { p.muted };
    let text = if active {
        format!("{label} {}", if state.descending { icons::CARET_DOWN } else { icons::CARET_UP })
    } else {
        label.to_string()
    };
    let (pos, align) = if right {
        (Pos2::new(rect.right() - 8.0, rect.center().y), Align2::RIGHT_CENTER)
    } else {
        (Pos2::new(rect.left(), rect.center().y), Align2::LEFT_CENTER)
    };
    ui.painter().text(pos, align, text, semibold(12.5), color);
}

struct Columns {
    check: Rect,
    name: Rect,
    folder: Rect,
    tag: Option<Rect>,
    size: Rect,
    date: Rect,
}

fn columns(row: Rect, with_tag: bool) -> Columns {
    let (left, right) = (row.left(), row.right() - 10.0);
    let span = |x0: f32, x1: f32| Rect::from_min_max(Pos2::new(x0, row.top()), Pos2::new(x1, row.bottom()));
    let date_x = right - 92.0;
    let size_x = date_x - 118.0;
    let tag_w = if with_tag { 170.0 } else { 0.0 };
    let tag_x = size_x - tag_w;
    let flexible = (tag_x - (left + 36.0)).max(120.0);
    let name_w = (flexible * 0.48).max(90.0);
    Columns {
        check: span(left, left + 36.0),
        name: span(left + 36.0, left + 36.0 + name_w - 10.0),
        folder: span(left + 36.0 + name_w, tag_x - 10.0),
        tag: with_tag.then(|| span(tag_x, size_x - 8.0)),
        size: span(size_x, date_x - 8.0),
        date: span(date_x, right),
    }
}

/// Dessine la table. `tag` ajoute une colonne libre (`tag_title`).
pub fn file_table(
    ui: &mut Ui,
    env: &mut Env<'_>,
    state: &mut TableState,
    rows: &[FileId],
    tag_title: Option<&str>,
    tag: Option<&dyn Fn(FileId) -> String>,
) {
    let p = pal(ui);
    state.refresh(env.scan, rows, tag);
    let with_tag = tag.is_some();
    let lang = lang();
    let visible_total = state.order.len();
    let max_size = state.order.iter().map(|&id| env.scan.file(id).size).max().unwrap_or(1).max(1);

    // En-tete
    let (header, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::hover());
    let cols = columns(header, with_tag);
    let all_selected = visible_total > 0 && state.order.iter().all(|id| env.sel.files.contains(id));
    let some_selected = !all_selected && state.order.iter().any(|id| env.sel.files.contains(id));
    let check_response = ui.interact(cols.check, ui.scope_id().with("all"), Sense::click());
    paint_check(ui, ui.painter(), cols.check.center(), all_selected, some_selected, check_response.hovered());
    if check_response.clicked() {
        let target = !all_selected;
        for &id in &state.order.clone() {
            env.sel.set(env.scan, id, target);
        }
    }
    sort_header(ui, state, cols.name, SortKey::Name, t("Nom", "Name"), false);
    sort_header(ui, state, cols.folder, SortKey::Folder, t("Dossier", "Folder"), false);
    if let (Some(rect), Some(title)) = (cols.tag, tag_title) {
        sort_header(ui, state, rect, SortKey::Tag, title, false);
    }
    sort_header(ui, state, cols.size, SortKey::Size, t("Taille", "Size"), true);
    sort_header(ui, state, cols.date, SortKey::Modified, t("Modifié", "Modified"), false);
    ui.painter().hline(header.x_range(), header.bottom(), egui::Stroke::new(1.0, p.line));
    state.refresh(env.scan, rows, tag);

    // Clavier
    let typing = ui.ctx().egui_wants_keyboard_input();
    if !typing && visible_total > 0 {
        let (mut delta, space) = ui.input_mut(|i| {
            let mut delta = 0i64;
            if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                delta = 1;
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                delta = -1;
            }
            if i.consume_key(Modifiers::NONE, Key::PageDown) {
                delta = 20;
            }
            if i.consume_key(Modifiers::NONE, Key::PageUp) {
                delta = -20;
            }
            (delta, i.consume_key(Modifiers::NONE, Key::Space))
        });
        if delta != 0 {
            let current = state.cursor.map_or(-1, |c| c as i64);
            if current < 0 && delta < 0 {
                delta = 1;
            }
            let next = (current + delta).clamp(0, visible_total as i64 - 1) as usize;
            state.cursor = Some(next);
            state.scroll_to_cursor = true;
            *env.focus = Some(state.order[next]);
        }
        if space
            && let Some(cursor) = state.cursor {
                env.sel.toggle(env.scan, state.order[cursor]);
            }
        if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::A)) {
            for &id in &state.order.clone() {
                env.sel.set(env.scan, id, true);
            }
        }
    }

    let row_height = env.row_height;
    let mut area = egui::ScrollArea::vertical().auto_shrink(false).id_salt("rows");
    if state.scroll_to_cursor {
        if let Some(cursor) = state.cursor {
            let viewport = ui.available_height();
            let offset = (cursor as f32 * row_height) - viewport / 2.0;
            area = area.vertical_scroll_offset(offset.max(0.0));
        }
        state.scroll_to_cursor = false;
    }
    area.show_rows(ui, row_height, visible_total, |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for index in range {
            let id = state.order[index];
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), row_height), Sense::click());
            let file = env.scan.file(id);
            let checked = env.sel.files.contains(&id);
            let focused = *env.focus == Some(id);
            let painter = ui.painter();
            if focused {
                painter.rect_filled(rect, CornerRadius::same(6), p.accent_soft);
            } else if checked || response.hovered() {
                painter.rect_filled(rect, CornerRadius::same(6), p.hover);
            }
            let cols = columns(rect, with_tag);
            let pointer = response.interact_pointer_pos();
            let on_check = pointer.is_some_and(|pos| cols.check.contains(pos));
            let hover_check = response.hover_pos().is_some_and(|pos| cols.check.contains(pos));
            paint_check(ui, painter, cols.check.center(), checked, false, hover_check);

            let y = rect.center().y;
            let icon_color = category_color(file.category as usize);
            painter.text(Pos2::new(cols.name.left() + 8.0, y), Align2::CENTER_CENTER, file_icon(env.scan, file), body(16.0), icon_color);
            let mut name_width = cols.name.width() - 24.0;
            if file.has(flags::CLOUD) {
                painter.text(Pos2::new(cols.name.right() - 8.0, y), Align2::CENTER_CENTER, icons::CLOUD, body(13.0), p.faint);
                name_width -= 20.0;
            }
            paint_text(ui, painter, Pos2::new(cols.name.left() + 24.0, y), &file.name, body(14.0), p.text, name_width);

            let shown = short_folder(env.scan, env.scan.folder_of(id));
            paint_text(ui, painter, Pos2::new(cols.folder.left(), y), &shown, body(13.0), p.muted, cols.folder.width());

            if let (Some(tag), Some(rect)) = (tag, cols.tag) {
                paint_text(ui, painter, Pos2::new(rect.left(), y), &tag(id), body(12.5), p.muted, rect.width());
            }

            let size_text = format_size(file.size, lang);
            painter.text(Pos2::new(cols.size.right(), y - 3.0), Align2::RIGHT_CENTER, size_text, body(13.0), p.text);
            let bar = Rect::from_min_size(Pos2::new(cols.size.right() - 56.0, y + 8.0), Vec2::new(56.0, 2.0));
            meter(ui, painter, bar, (file.size as f64 / max_size as f64).sqrt() as f32, icon_color);
            paint_text(ui, painter, Pos2::new(cols.date.left(), y), &format_date(file.modified, lang), body(13.0), p.muted, cols.date.width());

            if response.clicked() {
                let shift = ui.input(|i| i.modifiers.shift);
                let ctrl = ui.input(|i| i.modifiers.command);
                if shift {
                    let anchor = state.anchor.unwrap_or(index);
                    let (a, b) = (anchor.min(index), anchor.max(index));
                    for &row in &state.order[a..=b] {
                        env.sel.set(env.scan, row, true);
                    }
                } else if on_check || ctrl {
                    env.sel.toggle(env.scan, id);
                    state.anchor = Some(index);
                } else {
                    *env.focus = Some(id);
                    state.anchor = Some(index);
                }
                state.cursor = Some(index);
            }
            if response.double_clicked() {
                env.actions.push(Action::OpenPath(env.scan.file_path(id)));
            }
            response.context_menu(|ui| file_menu(ui, env, id));
        }
    });
}
