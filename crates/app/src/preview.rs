//! Inspecteur de fichier : apercu, proprietes, actions rapides.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, ColorImage, CornerRadius, Frame, Margin, RichText, TextureHandle, TextureOptions, Ui, Vec2};
use filecleaner_core::config::category_label;
use filecleaner_core::model::{Scan, flags};
use filecleaner_core::util::{format_age, format_datetime, format_size};
use image::ImageReader;

use crate::app::{Action, App};
use crate::icons;
use crate::jobs::{JobResult, Jobs};
use crate::table::file_icon;
use crate::theme::{category_color, lang, pal, semibold, t};
use crate::widgets::{Kind, button, icon_button};

const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "gif", "tif", "tiff", "ico"];
const TEXT_EXTS: &[&str] = &[
    "txt", "md", "log", "csv", "json", "xml", "yaml", "yml", "toml", "ini", "cfg", "conf", "py", "js",
    "ts", "tsx", "jsx", "rs", "c", "h", "cpp", "hpp", "cs", "java", "go", "rb", "php", "html", "css",
    "scss", "sh", "ps1", "bat", "sql",
];
const MAX_TEXTURES: usize = 192;
const TEXT_PREVIEW_BYTES: usize = 8 * 1024;

#[derive(Default)]
pub struct Previews {
    textures: HashMap<PathBuf, Option<TextureHandle>>,
    order: VecDeque<PathBuf>,
    pending: HashSet<PathBuf>,
    texts: HashMap<PathBuf, Option<String>>,
}

fn decode_thumbnail(path: &Path, max: u32) -> Option<Arc<ColorImage>> {
    let image = ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?;
    let thumb = image.thumbnail(max, max).to_rgba8();
    let size = [thumb.width() as usize, thumb.height() as usize];
    Some(Arc::new(ColorImage::from_rgba_unmultiplied(size, thumb.as_raw())))
}

fn read_text(path: &Path) -> Option<String> {
    let mut buffer = vec![0u8; TEXT_PREVIEW_BYTES];
    let n = std::fs::File::open(path).ok()?.read(&mut buffer).ok()?;
    buffer.truncate(n);
    if buffer.contains(&0) {
        return None;
    }
    Some(String::from_utf8_lossy(&buffer).into_owned())
}

impl Previews {
    /// Retourne la miniature si elle est prete, sinon la demande en arriere-plan.
    pub fn thumbnail(&mut self, jobs: &Jobs, path: &Path) -> Option<TextureHandle> {
        if let Some(entry) = self.textures.get(path) {
            return entry.clone();
        }
        if self.pending.insert(path.to_path_buf()) {
            let (tx, ctx) = jobs.sender();
            let path = path.to_path_buf();
            rayon::spawn(move || {
                let image = decode_thumbnail(&path, 360);
                let _ = tx.send(JobResult::Thumbnail(path, image));
                ctx.request_repaint();
            });
        }
        None
    }

    pub fn store_thumbnail(&mut self, ctx: &egui::Context, path: PathBuf, image: Option<Arc<ColorImage>>) {
        self.pending.remove(&path);
        let texture = image.map(|image| ctx.load_texture(path.to_string_lossy(), image, TextureOptions::LINEAR));
        self.textures.insert(path.clone(), texture);
        self.order.push_back(path);
        while self.order.len() > MAX_TEXTURES {
            if let Some(oldest) = self.order.pop_front() {
                self.textures.remove(&oldest);
            }
        }
    }

    fn text(&mut self, jobs: &Jobs, path: &Path) -> Option<&str> {
        if !self.texts.contains_key(path) && self.pending.insert(path.to_path_buf()) {
            let path = path.to_path_buf();
            jobs.quiet(move || {
                let text = read_text(&path);
                JobResult::TextPreview(path, text)
            });
        }
        self.texts.get(path)?.as_deref()
    }

    pub fn store_text(&mut self, path: PathBuf, text: Option<String>) {
        self.pending.remove(&path);
        if self.texts.len() > 64 {
            self.texts.clear();
        }
        self.texts.insert(path, text);
    }
}

fn property(ui: &mut Ui, label: &str, value: &str) {
    let p = pal(ui);
    ui.horizontal_top(|ui| {
        ui.allocate_ui(Vec2::new(82.0, 18.0), |ui| {
            ui.set_min_width(82.0);
            ui.label(RichText::new(label).size(13.0).color(p.muted));
        });
        ui.add(egui::Label::new(RichText::new(value).size(13.0).color(p.text)).wrap());
    });
}

pub fn inspector(ui: &mut Ui, app: &mut App, scan: &Scan) {
    let Some(id) = app.focus else { return };
    let p = pal(ui);
    let file = scan.file(id);
    let path = scan.file_path(id);
    let ext = scan.ext_of(file);
    let lang = lang();

    ui.horizontal(|ui| {
        ui.label(RichText::new(t("Détails", "Details")).font(semibold(14.0)).color(p.muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icon_button(ui, icons::X, t("Fermer (Échap)", "Close (Esc)")).clicked() {
                app.focus = None;
            }
        });
    });

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        let width = ui.available_width();
        let cloud = file.has(flags::CLOUD);
        Frame::new().fill(p.raised).corner_radius(CornerRadius::same(10)).inner_margin(Margin::same(8)).show(ui, |ui| {
            ui.set_width(width - 16.0);
            ui.set_min_height(150.0);
            let mut shown = false;
            if !cloud && IMAGE_EXTS.contains(&ext) {
                if let Some(texture) = app.previews.thumbnail(&app.jobs, &path) {
                    let size = texture.size_vec2();
                    let scale = ((width - 16.0) / size.x).min(240.0 / size.y).min(1.5);
                    ui.vertical_centered(|ui| {
                        ui.add(egui::Image::new((texture.id(), size * scale)).corner_radius(6));
                    });
                    shown = true;
                }
            } else if !cloud && TEXT_EXTS.contains(&ext)
                && let Some(text) = app.previews.text(&app.jobs, &path) {
                    let excerpt: String = text.lines().take(14).collect::<Vec<_>>().join("\n");
                    ui.add(egui::Label::new(RichText::new(excerpt).monospace().size(11.5).color(p.muted)).truncate());
                    shown = true;
                }
            if !shown {
                ui.vertical_centered(|ui| {
                    ui.add_space(34.0);
                    ui.label(RichText::new(file_icon(scan, file)).size(52.0).color(category_color(file.category as usize)));
                });
            }
        });
        ui.add_space(4.0);
        ui.add(egui::Label::new(RichText::new(&*file.name).font(semibold(15.0)).color(p.text)).wrap());
        ui.label(RichText::new(format_size(file.size, lang)).color(p.muted));
        ui.add_space(4.0);

        ui.horizontal_wrapped(|ui| {
            let selected = app.sel.files.contains(&id);
            let label = if selected { t("Retirer", "Remove") } else { t("Sélectionner", "Select") };
            if button(ui, Kind::Primary, label).clicked() {
                app.sel.toggle(scan, id);
            }
            if button(ui, Kind::Secondary, t("Ouvrir", "Open")).clicked() {
                app.actions.push(Action::OpenPath(path.clone()));
            }
            if button(ui, Kind::Secondary, t("Explorateur", "Explorer")).clicked() {
                app.actions.push(Action::Reveal(path.clone()));
            }
        });
        ui.add_space(4.0);
        ui.separator();
        ui.spacing_mut().item_spacing.y = 5.0;
        property(ui, t("Catégorie", "Category"), category_label(scan.category_of(file), lang));
        if !ext.is_empty() {
            property(ui, t("Extension", "Extension"), &format!(".{ext}"));
        }
        property(ui, t("Modifié", "Modified"), &format!("{} ({})", format_datetime(file.modified, lang), format_age(file.modified, lang)));
        property(ui, t("Créé", "Created"), &format_datetime(file.created, lang));
        property(ui, t("Ouvert", "Accessed"), &format_datetime(file.accessed, lang));
        property(ui, t("Dossier", "Folder"), &scan.folder_of(id).to_string_lossy());
        if cloud {
            property(ui, t("Stockage", "Storage"), t("Cloud uniquement", "Cloud only"));
        }
        if file.has(flags::HIDDEN) {
            property(ui, t("Attributs", "Attributes"), t("Masqué", "Hidden"));
        }
        if let Some(rule) = app.derived.as_ref().and_then(|d| d.junk_rules.get(&id)) {
            let label = filecleaner_core::analysis::rule_label(*rule).get(lang);
            property(ui, t("Détecté", "Detected"), label);
        }
    });
}
