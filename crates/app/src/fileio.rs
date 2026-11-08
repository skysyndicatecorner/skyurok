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

// ---------- замок файла и резервные копии ----------

/// Кто держит файл проекта открытым. Лежит рядом с проектом: «проект.json.lock».
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LockInfo {
    pub user: String,
    pub host: String,
    pub pid: u32,
    /// Когда открыт, «ГГГГ-ММ-ДД ЧЧ:ММ».
    pub since: String,
    /// Время последнего обновления, секунды Unix.
    pub ts: i64,
}

/// Замок старше этого срока считается брошенным (программа закрылась со сбоем).
const LOCK_STALE_SECS: i64 = 12 * 3600;

pub fn lock_path(project: &Path) -> PathBuf {
    let name = project.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "project".into());
    project.with_file_name(format!("{name}.lock"))
}

fn host_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "неизвестный компьютер".into())
}

fn user_name() -> String {
    std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "пользователь".into())
}

pub fn me() -> LockInfo {
    let now = chrono::Local::now();
    LockInfo { user: user_name(), host: host_name(), pid: std::process::id(), since: now.format("%Y-%m-%d %H:%M").to_string(), ts: now.timestamp() }
}

/// Чужой действующий замок, если он есть. Замок с этого же компьютера считается
/// оставшимся от прошлого запуска и не мешает.
pub fn foreign_lock(project: &Path) -> Option<LockInfo> {
    let text = std::fs::read_to_string(lock_path(project)).ok()?;
    let info: LockInfo = serde_json::from_str(&text).ok()?;
    let fresh = chrono::Local::now().timestamp() - info.ts < LOCK_STALE_SECS;
    (fresh && info.host != host_name()).then_some(info)
}

pub fn take_lock(project: &Path) {
    if let Ok(t) = serde_json::to_string(&me()) {
        let _ = std::fs::write(lock_path(project), t);
    }
}

/// Снять замок, только если он наш.
pub fn release_lock(project: &Path) {
    let lp = lock_path(project);
    let ours = std::fs::read_to_string(&lp)
        .ok()
        .and_then(|t| serde_json::from_str::<LockInfo>(&t).ok())
        .is_some_and(|i| i.host == host_name() && i.pid == std::process::id());
    if ours {
        let _ = std::fs::remove_file(lp);
    }
}

pub fn mtime(p: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Сколько резервных копий хранить на каждый проект.
pub const BACKUPS_KEEP: usize = 30;

fn backup_dir(project: &Path) -> PathBuf {
    let stem: String = project
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".into())
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    data_dir().join("backups").join(stem)
}

/// Скопировать текущий файл проекта в резервные копии перед перезаписью.
pub fn backup(project: &Path) -> Result<(), String> {
    if !project.exists() {
        return Ok(());
    }
    let dir = backup_dir(project);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    std::fs::copy(project, dir.join(format!("{stamp}.json"))).map_err(|e| e.to_string())?;
    let mut all = backups(project);
    while all.len() > BACKUPS_KEEP {
        if let Some(old) = all.pop() {
            let _ = std::fs::remove_file(old.0);
        }
    }
    Ok(())
}

/// Резервные копии проекта, новые первыми: путь, «ГГГГ-ММ-ДД ЧЧ:ММ:СС», размер.
pub fn backups(project: &Path) -> Vec<(PathBuf, String, u64)> {
    let Ok(rd) = std::fs::read_dir(backup_dir(project)) else { return vec![] };
    let mut v: Vec<(PathBuf, String, u64)> = rd
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .map(|e| {
            let p = e.path();
            let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let label = match stem.split_once('_') {
                Some((d, t)) => format!("{d} {}", t.replace('-', ":")),
                None => stem,
            };
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            (p, label, size)
        })
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}
