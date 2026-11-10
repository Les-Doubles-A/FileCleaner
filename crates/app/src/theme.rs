//! Jetons de couleur, typographie et style global.

use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow,
    Stroke, TextStyle, Theme, Ui, Vec2,
};
use filecleaner_core::util::Lang;

static LANG: AtomicU8 = AtomicU8::new(0);
static ACCENT: AtomicU32 = AtomicU32::new(0x408A_FF00);

pub fn set_lang(lang: Lang) {
    LANG.store(u8::from(lang == Lang::En), Ordering::Relaxed);
}

pub fn lang() -> Lang {
    if LANG.load(Ordering::Relaxed) == 0 { Lang::Fr } else { Lang::En }
}

/// Texte selon la langue de l'interface.
pub fn t(fr: &'static str, en: &'static str) -> &'static str {
    if lang() == Lang::Fr { fr } else { en }
}

pub const RADIUS: u8 = 8;
pub const RADIUS_LG: u8 = 12;

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub sidebar: Color32,
    pub card: Color32,
    pub raised: Color32,
    pub hover: Color32,
    pub line: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub on_accent: Color32,
    pub danger: Color32,
    pub danger_soft: Color32,
    pub warn: Color32,
    pub warn_soft: Color32,
    pub ok: Color32,
    pub ok_soft: Color32,
}

fn soft(color: Color32, dark: bool) -> Color32 {
    let alpha = if dark { 46 } else { 30 };
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

pub fn palette(dark: bool) -> Palette {
    let raw = ACCENT.load(Ordering::Relaxed);
    let accent = Color32::from_rgb((raw >> 24) as u8, (raw >> 16) as u8, (raw >> 8) as u8);
    let luminance = 0.299 * f32::from(accent.r()) + 0.587 * f32::from(accent.g()) + 0.114 * f32::from(accent.b());
    let on_accent = if luminance > 165.0 { Color32::from_rgb(16, 18, 22) } else { Color32::WHITE };
    if dark {
        let (danger, warn, ok) = (
            Color32::from_rgb(244, 98, 94),
            Color32::from_rgb(240, 177, 66),
            Color32::from_rgb(66, 196, 132),
        );
        Palette {
            dark,
            bg: Color32::from_rgb(17, 19, 24),
            sidebar: Color32::from_rgb(13, 15, 19),
            card: Color32::from_rgb(24, 27, 33),
            raised: Color32::from_rgb(31, 35, 43),
            hover: Color32::from_rgb(37, 41, 50),
            line: Color32::from_rgb(40, 45, 54),
            text: Color32::from_rgb(232, 234, 238),
            muted: Color32::from_rgb(158, 165, 177),
            faint: Color32::from_rgb(112, 120, 133),
            accent,
            accent_soft: soft(accent, true),
            on_accent,
            danger,
            danger_soft: soft(danger, true),
            warn,
            warn_soft: soft(warn, true),
            ok,
            ok_soft: soft(ok, true),
        }
    } else {
        let (danger, warn, ok) = (
            Color32::from_rgb(208, 56, 52),
            Color32::from_rgb(176, 112, 8),
            Color32::from_rgb(20, 134, 84),
        );
        Palette {
            dark,
            bg: Color32::from_rgb(245, 246, 248),
            sidebar: Color32::from_rgb(236, 238, 242),
            card: Color32::WHITE,
            raised: Color32::from_rgb(248, 249, 251),
            hover: Color32::from_rgb(232, 235, 240),
            line: Color32::from_rgb(222, 225, 231),
            text: Color32::from_rgb(26, 29, 35),
            muted: Color32::from_rgb(88, 96, 110),
            faint: Color32::from_rgb(130, 138, 152),
            accent,
            accent_soft: soft(accent, false),
            on_accent,
            danger,
            danger_soft: soft(danger, false),
            warn,
            warn_soft: soft(warn, false),
            ok,
            ok_soft: soft(ok, false),
        }
    }
}

pub fn pal(ui: &Ui) -> Palette {
    palette(ui.visuals().dark_mode)
}

/// Couleurs de categories, reparties sur la roue chromatique.
const CATEGORY_COLORS: [[u8; 3]; 13] = [
    [79, 142, 247],
    [226, 94, 151],
    [160, 108, 240],
    [63, 184, 175],
    [235, 104, 83],
    [228, 164, 58],
    [90, 190, 110],
    [120, 130, 240],
    [197, 140, 80],
    [150, 170, 70],
    [90, 175, 225],
    [205, 110, 205],
    [135, 145, 160],
];

pub fn category_color(index: usize) -> Color32 {
    let [r, g, b] = CATEGORY_COLORS[index % CATEGORY_COLORS.len()];
    Color32::from_rgb(r, g, b)
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

pub fn body(size: f32) -> FontId {
    FontId::proportional(size)
}

/// Polices systeme si disponibles, polices integrees d'egui sinon.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "phosphor".into(),
        FontData::from_static(include_bytes!("../../../assets/fonts/Phosphor.ttf")).into(),
    );
    let windows = std::env::var_os("SystemRoot").map(std::path::PathBuf::from).unwrap_or_default().join("Fonts");
    let load = |file: &str| std::fs::read(windows.join(file)).ok();

    let mut proportional = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut bold_family = proportional.clone();
    if let Some(bytes) = load("segoeui.ttf") {
        fonts.font_data.insert("segoe".into(), FontData::from_owned(bytes).into());
        proportional.insert(0, "segoe".into());
    }
    if let Some(bytes) = load("seguisb.ttf") {
        fonts.font_data.insert("segoe-semibold".into(), FontData::from_owned(bytes).into());
        bold_family.insert(0, "segoe-semibold".into());
    }
    // L'icone vient juste apres la police de texte : Phosphor mappe a-z sur des glyphes vides.
    proportional.insert(1.min(proportional.len()), "phosphor".into());
    bold_family.insert(1.min(bold_family.len()), "phosphor".into());
    fonts.families.insert(FontFamily::Proportional, proportional);
    fonts.families.insert(FontFamily::Name("semibold".into()), bold_family);
    if let Some(bytes) = load("consola.ttf") {
        fonts.font_data.insert("consolas".into(), FontData::from_owned(bytes).into());
        if let Some(mono) = fonts.families.get_mut(&FontFamily::Monospace) {
            mono.insert(0, "consolas".into());
        }
    }
    ctx.set_fonts(fonts);
}

fn visuals(p: Palette) -> egui::Visuals {
    let mut v = if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = p.bg;
    v.window_fill = p.card;
    v.faint_bg_color = p.raised;
    v.extreme_bg_color = p.raised;
    v.code_bg_color = p.raised;
    v.window_stroke = Stroke::new(1.0, p.line);
    v.window_corner_radius = CornerRadius::same(RADIUS_LG);
    v.menu_corner_radius = CornerRadius::same(RADIUS);
    v.window_shadow = Shadow {
        offset: [0, 10],
        blur: 32,
        spread: 0,
        color: Color32::from_black_alpha(if p.dark { 110 } else { 40 }),
    };
    v.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(if p.dark { 90 } else { 30 }),
    };
    v.override_text_color = None;
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent_soft;
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.text_cursor.stroke = Stroke::new(1.5, p.accent);
    v.striped = false;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.indent_has_left_vline = false;

    let radius = CornerRadius::same(RADIUS);
    let w = &mut v.widgets;
    for state in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
    w.noninteractive.bg_fill = p.card;
    w.noninteractive.weak_bg_fill = p.card;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.line);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.bg_fill = p.raised;
    w.inactive.weak_bg_fill = p.raised;
    w.inactive.bg_stroke = Stroke::new(1.0, p.line);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.hovered.bg_fill = p.hover;
    w.hovered.weak_bg_fill = p.hover;
    w.hovered.bg_stroke = Stroke::new(1.0, p.faint);
    w.hovered.fg_stroke = Stroke::new(1.0, p.text);
    w.active.bg_fill = p.hover;
    w.active.weak_bg_fill = p.hover;
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    w.active.fg_stroke = Stroke::new(1.0, p.text);
    w.open.bg_fill = p.hover;
    w.open.weak_bg_fill = p.hover;
    w.open.bg_stroke = Stroke::new(1.0, p.line);
    w.open.fg_stroke = Stroke::new(1.0, p.text);
    v
}

pub fn apply(ctx: &egui::Context, accent: [u8; 3]) {
    let packed = (u32::from(accent[0]) << 24) | (u32::from(accent[1]) << 16) | (u32::from(accent[2]) << 8);
    ACCENT.store(packed, Ordering::Relaxed);
    ctx.set_visuals_of(Theme::Dark, visuals(palette(true)));
    ctx.set_visuals_of(Theme::Light, visuals(palette(false)));
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, body(12.0)),
            (TextStyle::Body, body(14.0)),
            (TextStyle::Button, body(14.0)),
            (TextStyle::Heading, semibold(22.0)),
            (TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();
        let s = &mut style.spacing;
        s.item_spacing = Vec2::new(8.0, 8.0);
        s.button_padding = Vec2::new(12.0, 6.0);
        s.interact_size = Vec2::new(32.0, 32.0);
        s.window_margin = Margin::same(20);
        s.menu_margin = Margin::same(6);
        s.scroll.bar_width = 8.0;
        s.scroll.floating = true;
        s.combo_height = 320.0;
        style.interaction.tooltip_delay = 0.35;
        style.animation_time = 0.12;
    });
}
