// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
#![allow(dead_code)]
//! Диалоги файлов и открытие документов — на Android подменяются папкой приложения.

use std::path::{Path, PathBuf};

/// Папка приложения на Android (задаётся при запуске из `android_main`).
pub static ANDROID_DATA: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

pub fn data_dir() -> PathBuf {
    let d = ANDROID_DATA.get().cloned().or_else(|| eframe::storage_dir(crate::APP_ID)).unwrap_or_else(|| std::env::temp_dir().join("skyurok"));
    let _ = std::fs::create_dir_all(&d);
    d
}

#[allow(dead_code)]
pub fn documents_dir() -> PathBuf {
    #[cfg(not(target_os = "android"))]
    {
        let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from);
        if let Some(h) = home {
            for sub in ["Documents", "Документы"] {
                let p = h.join(sub);
                if p.is_dir() {
                    return p.join("SkyUrok");
                }
            }
            return h.join("SkyUrok");
        }
    }
    data_dir().join("documents")
}

#[cfg(not(target_os = "android"))]
pub fn pick_open(name: &str, exts: &[&str]) -> Option<PathBuf> {
    rfd::FileDialog::new().add_filter(name, exts).pick_file()
}

#[cfg(not(target_os = "android"))]
pub fn pick_save(name: &str, exts: &[&str], default_name: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().add_filter(name, exts).set_file_name(default_name).save_file()
}

#[cfg(not(target_os = "android"))]
pub fn pick_folder() -> Option<PathBuf> {
    rfd::FileDialog::new().pick_folder()
}

#[cfg(target_os = "android")]
pub fn pick_open(_name: &str, exts: &[&str]) -> Option<PathBuf> {
    // На Android берём последний файл с нужным расширением из папки приложения.
    let dir = documents_dir();
    std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| exts.contains(&e))).max()
}

#[cfg(target_os = "android")]
pub fn pick_save(_name: &str, _exts: &[&str], default_name: &str) -> Option<PathBuf> {
    let d = documents_dir();
    let _ = std::fs::create_dir_all(&d);
    Some(d.join(default_name))
}

#[cfg(target_os = "android")]
pub fn pick_folder() -> Option<PathBuf> {
    Some(documents_dir())
}

/// Открыть файл или папку программой по умолчанию (Word, просмотрщик PDF, браузер).
pub fn open_external(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        // explorer открывает файл связанной программой и понимает кириллицу и пробелы.
        std::process::Command::new("explorer").arg(path).spawn().map(|_| ()).map_err(|e| e.to_string())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(path).spawn().map(|_| ()).map_err(|e| e.to_string())
    }
    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
    {
        for cmd in ["xdg-open", "gio"] {
            let mut c = std::process::Command::new(cmd);
            if cmd == "gio" {
                c.arg("open");
            }
            if c.arg(path).spawn().is_ok() {
                return Ok(());
            }
        }
        Err(format!("не удалось открыть {} — откройте файл вручную", path.display()))
    }
    #[cfg(target_os = "android")]
    {
        let _ = path;
        Err("На Android файл сохранён в папку приложения".into())
    }
}

pub fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}
