//! Reglages, enregistres a chaque changement.

use eframe::egui::{self, Color32, RichText, Sense, Stroke, Ui, Vec2};
use filecleaner_core::actions::DeleteMode;
use filecleaner_core::config::{Theme, category_label, default_categories};
use filecleaner_core::system;
use filecleaner_core::util::Lang;

use super::Page;
use crate::app::{Action, App};
use crate::dialogs::ToastKind;
use crate::icons;
use crate::jobs::PickPurpose;
use crate::theme::{self, lang, pal, t};
use crate::widgets::{Kind, button, card, icon_button, number, page_header, section_title, segmented, toggle};

#[derive(Default)]
pub struct SettingsState {
    pub confirm_purge: bool,
    ignored: Option<String>,
    junk: Option<String>,
    context_menu: Option<bool>,
}

const ACCENTS: [[u8; 3]; 6] = [
    [64, 138, 255],
    [124, 92, 246],
    [20, 170, 140],
    [226, 94, 151],
    [230, 130, 40],
    [90, 170, 70],
];

fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    let p = pal(ui);
    ui.horizontal(|ui| {
        ui.allocate_ui(Vec2::new(250.0, 30.0), |ui| {
            ui.set_min_width(250.0);
            ui.label(RichText::new(label).color(p.muted));
        });
        add(ui);
    });
}

fn split_list(text: &str) -> Vec<String> {
    text.split([',', '\n']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

pub fn show(ui: &mut Ui, app: &mut App) {
    let p = pal(ui);
    let lang_now = lang();
    page_header(ui, Page::Settings.title(), t("Les changements sont enregistrés automatiquement.", "Changes are saved automatically."));
    let before = app.settings.clone();

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        let width = ui.available_width().min(820.0);
        ui.set_max_width(width);

        card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let s = &mut app.settings;
            section_title(ui, t("Apparence", "Appearance"));
            row(ui, t("Thème", "Theme"), |ui| {
                segmented(ui, &mut s.theme, &[
                    (Theme::System, t("Système", "System")),
                    (Theme::Dark, t("Sombre", "Dark")),
                    (Theme::Light, t("Clair", "Light")),
                ]);
            });
            row(ui, t("Langue", "Language"), |ui| {
                segmented(ui, &mut s.language, &[(Lang::Fr, "Français"), (Lang::En, "English")]);
            });
            row(ui, t("Couleur d'accent", "Accent color"), |ui| {
                for color in ACCENTS {
                    let (rect, response) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
                    let fill = Color32::from_rgb(color[0], color[1], color[2]);
                    ui.painter().circle_filled(rect.center(), 10.0, fill);
                    if s.accent == color {
                        ui.painter().circle_stroke(rect.center(), 12.5, Stroke::new(2.0, fill));
                    }
                    if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        s.accent = color;
                    }
                }
            });
            row(ui, t("Taille de l'interface", "Interface size"), |ui| {
                let mut percent = (s.ui_scale * 100.0).round() as i32;
                if ui.add(egui::Slider::new(&mut percent, 80..=160).step_by(10.0).suffix(" %")).changed() {
                    s.ui_scale = percent as f32 / 100.0;
                }
            });
            toggle(ui, &mut s.compact_rows, t("Lignes compactes dans les listes", "Compact rows in lists"));
        });
        ui.add_space(4.0);

        card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, t("Sécurité", "Safety"));
            let s = &mut app.settings;
            row(ui, t("Destination par défaut", "Default destination"), |ui| {
                segmented(ui, &mut s.delete_mode, &[
                    (DeleteMode::Trash, t("Corbeille", "Recycle Bin")),
                    (DeleteMode::Quarantine, t("Quarantaine", "Quarantine")),
                ]);
            });
            row(ui, t("Purger la quarantaine après", "Purge quarantine after"), |ui| {
                let mut days = u64::from(s.quarantine_retention_days);
                if number(ui, &mut days, 0..=3650, t(" jours", " days")).changed() {
                    s.quarantine_retention_days = days as u32;
                }
                ui.label(RichText::new(t("0 = jamais", "0 = never")).size(12.5).color(p.faint));
            });
            let folder = s.quarantine_folder.clone().unwrap_or_else(|| t("Dans le dossier analysé", "Inside the scanned folder").to_string());
            let has_custom = s.quarantine_folder.is_some();
            let mut clear = false;
            row(ui, t("Dossier de quarantaine", "Quarantine folder"), |ui| {
                ui.label(RichText::new(folder).size(13.0).color(p.text));
                if button(ui, Kind::Ghost, t("Changer", "Change")).clicked() {
                    app.actions.push(Action::Pick(PickPurpose::Quarantine));
                }
                if has_custom && icon_button(ui, icons::X, t("Valeur par défaut", "Default value")).clicked() {
                    clear = true;
                }
            });
            if clear {
                s.quarantine_folder = None;
            }
        });
        ui.add_space(4.0);

        card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, t("Analyse", "Scanning"));
            let s = &mut app.settings;
            toggle(ui, &mut s.auto_duplicates, t("Chercher les doublons après chaque analyse", "Find duplicates after each scan"));
            toggle(ui, &mut s.skip_hidden, t("Ignorer les fichiers masqués et système", "Skip hidden and system files"));
            row(ui, t("Seuil « gros fichier »", "Large file threshold"), |ui| {
                number(ui, &mut s.big_file_threshold_mb, 1..=1_000_000, t(" Mo", " MB"));
            });
            row(ui, t("Seuil « fichier ancien »", "Old file threshold"), |ui| {
                let mut days = u64::from(s.old_file_threshold_days);
                if number(ui, &mut days, 1..=36_500, t(" jours", " days")).changed() {
                    s.old_file_threshold_days = days as u32;
                }
            });
            row(ui, t("Taille minimale des doublons", "Minimum duplicate size"), |ui| {
                let mut kb = s.min_duplicate_bytes / 1024;
                if number(ui, &mut kb, 0..=10_000_000, t(" Ko", " KB")).changed() {
                    s.min_duplicate_bytes = (kb * 1024).max(1);
                }
            });
            row(ui, t("Taille minimale analysée", "Minimum scanned size"), |ui| {
                let mut kb = s.min_size_bytes / 1024;
                if number(ui, &mut kb, 0..=10_000_000, t(" Ko", " KB")).changed() {
                    s.min_size_bytes = kb * 1024;
                }
            });
            row(ui, t("Taille maximale (0 = illimitée)", "Maximum size (0 = unlimited)"), |ui| {
                let mut mb = s.max_size_bytes / (1024 * 1024);
                if number(ui, &mut mb, 0..=100_000_000, t(" Mo", " MB")).changed() {
                    s.max_size_bytes = mb * 1024 * 1024;
                }
            });
            row(ui, t("Fils d'analyse (0 = auto)", "Scan threads (0 = auto)"), |ui| {
                let mut n = s.threads as u64;
                if number(ui, &mut n, 0..=64, "").changed() {
                    s.threads = n as usize;
                }
            });

            ui.add_space(4.0);
            ui.label(RichText::new(t("Extensions ignorées (séparées par des virgules)", "Ignored extensions (comma separated)")).color(p.muted));
            let ignored = app.pages.settings.ignored.get_or_insert_with(|| s.ignored_extensions.join(", "));
            if ui.add(egui::TextEdit::singleline(ignored).desired_width(f32::INFINITY).hint_text("iso, vhdx")).changed() {
                s.ignored_extensions = split_list(ignored);
            }
            ui.label(RichText::new(t("Motifs de fichiers inutiles personnalisés", "Custom junk file patterns")).color(p.muted));
            let junk = app.pages.settings.junk.get_or_insert_with(|| s.custom_junk_patterns.join(", "));
            if ui.add(egui::TextEdit::singleline(junk).desired_width(f32::INFINITY).hint_text("*.foo, cache_*")).changed() {
                s.custom_junk_patterns = split_list(junk);
            }

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(t("Dossiers exclus", "Excluded folders")).color(p.muted));
                if button(ui, Kind::Ghost, &format!("{}  {}", icons::PLUS, t("Ajouter", "Add"))).clicked() {
                    app.actions.push(Action::Pick(PickPurpose::Exclude));
                }
            });
            let mut remove = None;
            for (index, folder) in s.excluded_folders.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(folder).size(13.0).color(p.text));
                    if icon_button(ui, icons::X, t("Retirer", "Remove")).clicked() {
                        remove = Some(index);
                    }
                });
            }
            if let Some(index) = remove {
                s.excluded_folders.remove(index);
            }
        });
        ui.add_space(4.0);

        card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, t("Catégories", "Categories"));
            let s = &mut app.settings;
            for category in &mut s.categories {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut category.enabled, "");
                    ui.allocate_ui(Vec2::new(140.0, 24.0), |ui| {
                        ui.set_min_width(140.0);
                        ui.label(RichText::new(category_label(&category.name, lang_now)).color(p.text));
                    });
                    let mut text = category.exts.join(", ");
                    if ui.add(egui::TextEdit::singleline(&mut text).desired_width(f32::INFINITY)).changed() {
                        category.exts = split_list(&text);
                    }
                });
            }
            if button(ui, Kind::Ghost, t("Rétablir les catégories d'origine", "Restore default categories")).clicked() {
                s.categories = default_categories();
            }
            ui.label(RichText::new(t("Prend effet à la prochaine analyse.", "Takes effect on the next scan.")).size(12.5).color(p.faint));
        });
        ui.add_space(4.0);

        card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, t("Intégration et données", "Integration and data"));
            let enabled = *app.pages.settings.context_menu.get_or_insert_with(system::context_menu_installed);
            let mut wanted = enabled;
            toggle(ui, &mut wanted, t("« Analyser avec File Cleaner » dans le menu de l'Explorateur", "\"Scan with File Cleaner\" in the Explorer menu"));
            if wanted != enabled {
                let label = t("Analyser avec File Cleaner", "Scan with File Cleaner");
                let result = std::env::current_exe()
                    .map_err(anyhow::Error::from)
                    .and_then(|exe| system::set_context_menu(wanted, &exe, label));
                match result {
                    Ok(()) => app.pages.settings.context_menu = Some(wanted),
                    Err(err) => app.toasts.push(crate::dialogs::Toast::new(ToastKind::Error, format!("{err:#}"), None)),
                }
            }
            ui.horizontal_wrapped(|ui| {
                if button(ui, Kind::Secondary, t("Ouvrir le dossier de données", "Open the data folder")).clicked() {
                    app.actions.push(Action::OpenPath(app.paths.data.clone()));
                }
                if button(ui, Kind::Secondary, t("Vider le cache d'empreintes", "Clear the hash cache")).clicked() {
                    if let Ok(mut cache) = app.jobs.hash_cache.lock() {
                        *cache = Default::default();
                    }
                    let _ = std::fs::remove_file(app.paths.hash_cache_file());
                    app.toasts.push(crate::dialogs::Toast::new(ToastKind::Info, t("Cache vidé.", "Cache cleared.").to_string(), None));
                }
            });
            ui.add_space(4.0);
            ui.label(RichText::new(format!("File Cleaner {} · {}", env!("CARGO_PKG_VERSION"), t("100 % hors ligne", "100% offline"))).size(12.5).color(p.faint));
        });
    });

    if app.settings != before {
        app.settings_dirty = true;
        theme::set_lang(app.settings.language);
        if app.settings.junk_related_changed(&before) {
            app.refresh_derived();
        }
    }
}
