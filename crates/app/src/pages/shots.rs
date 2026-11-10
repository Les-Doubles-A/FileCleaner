//! Parcours automatique de l'application (fonctionnalite `shots`, jamais livree) :
//! capture chaque page, puis enchaine un vrai nettoyage et son annulation.
//! `FC_SHOTS=sortie FC_ROOT=dossier-jetable FileCleaner`

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{self, ColorImage, ViewportCommand};
use filecleaner_core::actions::DeleteMode;

use super::Page;
use crate::app::{Action, App};
use crate::dialogs::Palette;

#[derive(Default)]
pub struct Shots {
    dir: Option<PathBuf>,
    index: usize,
    wait: u32,
    started: bool,
    similar_done: bool,
    step: usize,
}

const PAGES: [Page; 17] = [
    Page::Home, Page::Explore, Page::Files, Page::Duplicates, Page::Similar, Page::Junk, Page::Big,
    Page::Old, Page::Empty, Page::Rare, Page::Dev, Page::System, Page::Organize, Page::Rename,
    Page::Quarantine, Page::History, Page::Settings,
];

impl Shots {
    pub fn from_env() -> Self {
        Self { dir: std::env::var_os("FC_SHOTS").map(PathBuf::from), ..Self::default() }
    }
}

fn save(ctx: &egui::Context, path: PathBuf) {
    let callback = egui::ScreenshotCallback::new(move |image: Arc<ColorImage>| {
        let [w, h] = image.size;
        let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
        if let Some(buffer) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
            let _ = buffer.save(path);
        }
    });
    ctx.send_viewport_cmd(ViewportCommand::Screenshot(callback));
}

/// Scenario de bout en bout, une etape par palier.
fn scenario(ctx: &egui::Context, app: &mut App, dir: &std::path::Path, suffix: &str) -> bool {
    let shot = |ctx: &egui::Context, name: &str| save(ctx, dir.join(format!("zz-{name}{suffix}.png")));
    let live = |app: &App| app.scan.as_ref().map_or(0, |s| s.total_files());
    match app.shots.step {
        0 => {
            app.command_palette = Some(Palette::default());
        }
        1 => shot(ctx, "palette"),
        2 => {
            app.command_palette = None;
            app.actions.push(Action::Navigate(Page::Junk));
        }
        3 => {
            if let Some(scan) = app.scan.clone() {
                let id = scan.alive().find(|(_, f)| f.size > 100_000).map(|(id, _)| id);
                app.focus = id;
            }
            app.actions.push(Action::ExpressClean);
        }
        4 => {
            shot(ctx, "review");
            if let Some(review) = &mut app.review {
                review.mode = DeleteMode::Quarantine;
            }
        }
        5 => {
            eprintln!("avant nettoyage : {} fichiers", live(app));
            if let Some(review) = app.review.take() {
                app.run_delete(review.title, review.targets, review.mode);
            }
        }
        6 => {
            shot(ctx, "after-clean");
            eprintln!("apres nettoyage : {} fichiers, quarantaine {}", live(app), app.history.quarantined().count());
            app.actions.push(Action::Navigate(Page::Quarantine));
        }
        7 => {
            shot(ctx, "quarantine");
            app.actions.push(Action::UndoLast);
        }
        8 => {
            eprintln!("apres annulation : {} fichiers, quarantaine {}", live(app), app.history.quarantined().count());
            app.actions.push(Action::Navigate(Page::History));
        }
        9 => shot(ctx, "history"),
        _ => return true,
    }
    app.shots.step += 1;
    false
}

pub fn tick(ctx: &egui::Context, app: &mut App) {
    let Some(dir) = app.shots.dir.clone() else { return };
    ctx.request_repaint();
    if !app.shots.started {
        app.shots.started = true;
        if let Some(root) = std::env::var_os("FC_ROOT") {
            app.actions.push(Action::Scan(vec![PathBuf::from(root)]));
        }
        return;
    }
    let fresh = app.derived.as_ref().map(|d| d.generation) == app.scan.as_ref().map(|s| s.generation);
    if app.is_busy() || !fresh {
        return;
    }
    if !app.shots.similar_done {
        app.shots.similar_done = true;
        app.actions.push(Action::FindSimilar);
        return;
    }
    if app.shots.wait < 25 {
        app.shots.wait += 1;
        return;
    }
    app.shots.wait = 0;
    let suffix = std::env::var("FC_SUFFIX").unwrap_or_default();
    if app.shots.index == PAGES.len() {
        if scenario(ctx, app, &dir, &suffix) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        return;
    }
    let page = PAGES[app.shots.index];
    if app.page == page {
        save(ctx, dir.join(format!("{:02}-{page:?}{suffix}.png", app.shots.index)));
        app.shots.index += 1;
        if let Some(next) = PAGES.get(app.shots.index) {
            app.actions.push(Action::Navigate(*next));
        }
    } else {
        app.actions.push(Action::Navigate(page));
    }
}
