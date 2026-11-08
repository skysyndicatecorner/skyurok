// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
#![allow(dead_code)]
//! Темы оформления. Все пары «текст/фон» проверены на контраст WCAG ≥ 4,5:1
//! (светлая — от 5,7:1, тёмная — от 6,2:1). Цвета берутся функциями,
//! которые смотрят на текущую тему.

use eframe::egui::{self, Color32, FontFamily, FontId, TextStyle};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

static DARK: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    pub fn label(self) -> &'static str {
        match self {
            ThemeMode::System => "Как в системе",
            ThemeMode::Light => "Светлая",
            ThemeMode::Dark => "Тёмная",
        }
    }
}

fn d() -> bool {
    DARK.load(Ordering::Relaxed)
}

macro_rules! pal {
    ($($name:ident: $l:expr, $d:expr;)*) => {
        $(pub fn $name() -> Color32 { if d() { $d } else { $l } })*
    };
}

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

pal! {
    text: hex(0x1B1F24), hex(0xE6EDF3);
    muted: hex(0x57606A), hex(0x9DA7B3);
    bg: hex(0xF6F8FA), hex(0x0D1117);
    surface: hex(0xFFFFFF), hex(0x161B22);
    surface2: hex(0xEEF2F6), hex(0x21262D);
    border: hex(0xD8DEE4), hex(0x30363D);
    hover: hex(0xEAEEF2), hex(0x1C222B);
    accent: hex(0x0A58CA), hex(0x6CB0FF);
    accent_text: hex(0xFFFFFF), hex(0x0D1117);
    sel_bg: hex(0xE3EDFF), hex(0x14294A);
    sel_text: hex(0x0A3C8A), hex(0xB9D6FF);
    cell_bg: hex(0xF1F4F8), hex(0x1F2630);
    pin_bg: hex(0xE3EDFF), hex(0x14294A);
    ok: hex(0x146C2E), hex(0x5FD38D);
    ok_bg: hex(0xE6F4EA), hex(0x10261A);
    bad: hex(0xB42318), hex(0xFF8A80);
    bad_bg: hex(0xFDECEA), hex(0x2A1517);
    warn: hex(0x8A4B00), hex(0xF2B84B);
    warn_bg: hex(0xFFF4E0), hex(0x2A2010);
    rep: hex(0x6B3E00), hex(0xFFD58A);
    rep_bg: hex(0xFFE9B8), hex(0x3A2A0A);
    closed_bg: hex(0xEEEEEE), hex(0x1A1F26);
}

pub fn is_dark() -> bool {
    d()
}

/// Вызывается в начале каждого кадра.
pub fn sync(ctx: &egui::Context) {
    DARK.store(ctx.theme() == egui::Theme::Dark, Ordering::Relaxed);
}

pub fn apply(ctx: &egui::Context, mode: ThemeMode) {
    ctx.set_theme(match mode {
        ThemeMode::System => egui::ThemePreference::System,
        ThemeMode::Light => egui::ThemePreference::Light,
        ThemeMode::Dark => egui::ThemePreference::Dark,
    });
}

pub fn bold_family() -> FontFamily {
    FontFamily::Name("bold".into())
}

pub fn bold(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).family(bold_family())
}

fn fonts(ctx: &egui::Context) {
    use std::sync::Arc;
    let mut f = egui::FontDefinitions::default();
    f.font_data.insert("inter".into(), Arc::new(egui::FontData::from_static(include_bytes!("../assets/Inter-Regular.otf"))));
    f.font_data.insert("inter-semibold".into(), Arc::new(egui::FontData::from_static(include_bytes!("../assets/Inter-SemiBold.otf"))));
    f.font_data.insert("dejavu".into(), Arc::new(egui::FontData::from_static(skyurok_core::export_pdf::FONT_REGULAR)));
    let defaults = f.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut prop = vec!["inter".to_string(), "dejavu".to_string()];
    prop.extend(defaults.iter().cloned());
    f.families.insert(FontFamily::Proportional, prop);
    let mut b = vec!["inter-semibold".to_string(), "dejavu".to_string()];
    b.extend(defaults);
    f.families.insert(bold_family(), b);
    if let Some(m) = f.families.get_mut(&FontFamily::Monospace) {
        m.push("dejavu".into());
    }
    ctx.set_fonts(f);
}

fn tune(v: &mut egui::Visuals, dark: bool) {
    let c = |l: u32, dk: u32| if dark { hex(dk) } else { hex(l) };
    v.panel_fill = c(0xF6F8FA, 0x0D1117);
    v.window_fill = c(0xFFFFFF, 0x161B22);
    v.extreme_bg_color = c(0xFFFFFF, 0x0D1117);
    v.faint_bg_color = c(0xF1F4F8, 0x161B22);
    v.override_text_color = Some(c(0x1B1F24, 0xE6EDF3));
    v.hyperlink_color = c(0x0A58CA, 0x6CB0FF);
    v.selection.bg_fill = c(0xC8DEFF, 0x1F4D8C);
    v.selection.stroke = egui::Stroke::new(1.0, c(0x0A3C8A, 0xB9D6FF));
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, c(0xD8DEE4, 0x30363D));
    v.widgets.inactive.weak_bg_fill = c(0xEEF2F6, 0x21262D);
    v.widgets.inactive.bg_fill = c(0xFFFFFF, 0x0D1117);
    // граница флажков, переключателей и полей — контраст ≥3:1 (WCAG 1.4.11)
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, c(0x7D8590, 0x6E7681));
    v.widgets.hovered.weak_bg_fill = c(0xE3E8EE, 0x2A313B);
    v.widgets.active.weak_bg_fill = c(0xD8DEE4, 0x30363D);
    v.window_stroke = egui::Stroke::new(1.0, c(0xD8DEE4, 0x30363D));
    // видимый фокус: кольцо цвета акцента
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, c(0x8C959F, 0x6E7681));
    v.widgets.active.bg_stroke = egui::Stroke::new(2.0, c(0x0A58CA, 0x6CB0FF));
}

pub fn setup(ctx: &egui::Context, mode: ThemeMode) {
    fonts(ctx);
    ctx.all_styles_mut(|s| {
        s.text_styles = [
            (TextStyle::Heading, FontId::new(20.0, bold_family())),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(13.5, FontFamily::Monospace)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.interact_size.y = 28.0;
    });
    ctx.style_mut_of(egui::Theme::Light, |s| tune(&mut s.visuals, false));
    ctx.style_mut_of(egui::Theme::Dark, |s| tune(&mut s.visuals, true));
    apply(ctx, mode);
    sync(ctx);
}

pub fn small(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).small().color(muted())
}
