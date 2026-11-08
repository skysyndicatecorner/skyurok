// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
//! SkyUrok — настольное и мобильное приложение (egui).

mod app;
mod fileio;
mod onboarding;
mod palette;
mod tab_lists;
mod tab_loads;
mod tab_print;
mod tab_school;
mod tab_subs;
mod tab_timetable;
mod theme;
mod widgets;

pub use app::SkyApp;

pub const APP_ID: &str = "skyurok";

/// Записывает сбой в `crash.log` в папке данных: проект к этому моменту уже
/// сохранён автосохранением, а по журналу можно понять причину.
fn install_crash_log() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let dir = fileio::data_dir();
        let _ = std::fs::create_dir_all(&dir);
        let when = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let text = format!("[{when}] SkyUrok {} — сбой: {info}\n", env!("CARGO_PKG_VERSION"));
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("crash.log")) {
            let _ = f.write_all(text.as_bytes());
        }
        default(info);
    }));
}

#[cfg(not(target_os = "android"))]
pub fn run() -> eframe::Result {
    install_crash_log();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("SkyUrok")
            .with_app_id(APP_ID)
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([960.0, 600.0])
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png")).unwrap_or_default()),
        ..Default::default()
    };
    eframe::run_native(APP_ID, options, Box::new(|cc| Ok(Box::new(SkyApp::new(cc)))))
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    if let Some(p) = app.internal_data_path() {
        let _ = fileio::ANDROID_DATA.set(p);
    }
    install_crash_log();
    let options = eframe::NativeOptions { android_app: Some(app), ..Default::default() };
    let _ = eframe::run_native(APP_ID, options, Box::new(|cc| Ok(Box::new(SkyApp::new(cc)))));
}
