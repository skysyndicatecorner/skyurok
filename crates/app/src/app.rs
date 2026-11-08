// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::theme::ThemeMode;
use crate::{fileio, onboarding, palette, tab_lists, tab_loads, tab_print, tab_school, tab_subs, tab_timetable, theme, widgets};
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText};
use skyurok_core::model::*;
use skyurok_core::validate::{analyze, Report};
use skyurok_core::{demo, rtt};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    School,
    Lists,
    Loads,
    Timetable,
    Subs,
    Print,
}

#[derive(Clone)]
enum Pending {
    New,
    Demo,
    Open,
    OpenPath(PathBuf),
    ImportRtt,
    Restore,
    Quit,
}

pub struct SkyApp {
    pub project: Project,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub rev: u64,
    pub welcome: bool,
    undo: Vec<Project>,
    redo: Vec<Project>,
    pub tab: Tab,
    pub school_ui: tab_school::State,
    pub lists_ui: tab_lists::State,
    pub loads_ui: tab_loads::State,
    pub tt_ui: tab_timetable::State,
    pub subs_ui: tab_subs::State,
    pub print_ui: tab_print::State,
    status: Option<(String, bool, f64)>,
    recent: Vec<PathBuf>,
    last_autosave: f64,
    restore_offer: bool,
    confirm: Option<Pending>,
    about: bool,
    zoom: f32,
    report_cache: Option<(u64, Report)>,
    title: String,
    allow_close: bool,
    pub theme_mode: ThemeMode,
    pub onboarding: onboarding::State,
    pub palette: palette::State,
    pub onboarding_seen: bool,
    /// Файл открыт только для чтения: его держит другой компьютер.
    pub read_only: bool,
    lock_conflict: Option<(PathBuf, fileio::LockInfo, Box<Project>)>,
    loaded_mtime: Option<std::time::SystemTime>,
    overwrite_confirm: Option<PathBuf>,
    backups_open: bool,
}

pub fn empty_school() -> School {
    School {
        name: "Моя школа".into(),
        settings: Settings::default(),
        buildings: vec![Building {
            id: 1, name: "Основное здание".into(), address: String::new(), bells: default_bells(7), travel_minutes: vec![]
        }],
        rooms: vec![],
        subjects: vec![],
        teachers: vec![],
        classes: vec![],
        loads: vec![],
    }
}

pub fn default_bells(n: u8) -> Vec<BellTime> {
    let mut t = 8 * 60 + 30;
    (0..n)
        .map(|p| {
            let b = BellTime { period: p, start: format!("{:02}:{:02}", t / 60, t % 60), end: format!("{:02}:{:02}", (t + 45) / 60, (t + 45) % 60) };
            t += 45 + if p == 1 || p == 2 { 20 } else { 10 };
            b
        })
        .collect()
}

fn autosave_path() -> PathBuf {
    fileio::data_dir().join("autosave.json")
}
fn dirty_marker() -> PathBuf {
    fileio::data_dir().join("autosave.dirty")
}

impl SkyApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let zoom: f32 = cc.storage.and_then(|s| s.get_string("zoom")).and_then(|s| s.parse().ok()).filter(|z: &f32| (0.5..=3.0).contains(z)).unwrap_or(1.0);
        cc.egui_ctx.set_zoom_factor(zoom);
        let theme_mode: ThemeMode = cc.storage.and_then(|s| s.get_string("theme")).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let onboarding_seen = cc.storage.and_then(|s| s.get_string("onboarding_seen")).is_some_and(|s| s == "1");
        theme::setup(&cc.egui_ctx, theme_mode);
        let recent: Vec<PathBuf> = cc
            .storage
            .and_then(|s| s.get_string("recent"))
            .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
            .unwrap_or_default()
            .into_iter()
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .collect();
        let mut app = Self {
            project: Project::new(empty_school()),
            path: None,
            dirty: false,
            rev: 0,
            welcome: true,
            undo: vec![],
            redo: vec![],
            tab: Tab::Timetable,
            school_ui: Default::default(),
            lists_ui: Default::default(),
            loads_ui: Default::default(),
            tt_ui: Default::default(),
            subs_ui: Default::default(),
            print_ui: Default::default(),
            status: None,
            recent,
            last_autosave: 0.0,
            restore_offer: dirty_marker().exists() && autosave_path().exists(),
            confirm: None,
            about: false,
            zoom,
            report_cache: None,
            title: String::new(),
            allow_close: false,
            theme_mode,
            onboarding: Default::default(),
            palette: Default::default(),
            onboarding_seen,
            read_only: false,
            lock_conflict: None,
            loaded_mtime: None,
            overwrite_confirm: None,
            backups_open: false,
        };
        let arg = std::env::args().nth(1).map(PathBuf::from).filter(|p| p.exists());
        if let Some(p) = arg {
            app.restore_offer = false;
            if p.extension().is_some_and(|e| e == "rtt") {
                app.notify("Для .rtt используйте Файл → Импорт из другой программы", true);
            } else {
                app.open_path(p);
            }
        } else if !app.restore_offer {
            if let Some(p) = app.recent.first().cloned() {
                app.open_path(p);
            }
        }
        if let Ok(t) = std::env::var("SKYUROK_TAB") {
            app.tab = match t.as_str() {
                "school" => Tab::School,
                "lists" => Tab::Lists,
                "loads" => Tab::Loads,
                "subs" => Tab::Subs,
                "print" => Tab::Print,
                _ => Tab::Timetable,
            };
        }
        if let Ok(t) = std::env::var("SKYUROK_THEME") {
            let m = if t == "dark" { ThemeMode::Dark } else { ThemeMode::Light };
            app.set_theme(&cc.egui_ctx, m);
        }
        if let Ok(s) = std::env::var("SKYUROK_STEP") {
            app.onboarding.open_at(s.parse().unwrap_or(0));
        }
        if std::env::var("SKYUROK_PALETTE").is_ok() {
            app.palette.open();
        }
        if std::env::var("SKYUROK_VERSIONS").is_ok() {
            app.tt_ui.versions_open = true;
        }
        if std::env::var("SKYUROK_SOLVE").is_ok() {
            app.tab = Tab::Timetable;
            tab_timetable::start(&mut app, false);
        }
        if let Ok(s) = std::env::var("SKYUROK_EXPORT") {
            let f = if s == "pdf" { tab_print::Format::Pdf } else { tab_print::Format::Word };
            tab_print::start_export(&mut app, f);
        }
        if let Ok(d) = std::env::var("SKYUROK_DATE") {
            if let Some(d) = widgets::parse_date(&d) {
                app.subs_ui.set_date(d);
            }
        }
        app
    }

    // ---------- изменения и отмена ----------

    /// Любое изменение проекта — через эту функцию (снимок для отмены).
    pub fn edit(&mut self, f: impl FnOnce(&mut Project)) {
        if self.locked() {
            self.notify("Идёт составление расписания — изменения заблокированы до окончания", true);
            return;
        }
        if self.read_only {
            self.notify("Файл открыт только для чтения. Чтобы вносить изменения, сохраните копию: «Файл → Сохранить как…»", true);
            return;
        }
        self.undo.push(self.project.clone());
        if self.undo.len() > 60 {
            self.undo.remove(0);
        }
        self.redo.clear();
        f(&mut self.project);
        self.project.normalize();
        self.touch();
    }

    /// Во время генерации данные менять нельзя: результат посчитан по ним.
    pub fn palette_open(&self) -> bool {
        self.palette.is_open() || self.onboarding.is_open() || self.confirm.is_some() || self.tt_ui.versions_open
    }

    pub fn locked(&self) -> bool {
        self.tt_ui.job_running()
    }

    pub fn set_theme(&mut self, ctx: &egui::Context, m: ThemeMode) {
        self.theme_mode = m;
        theme::apply(ctx, m);
    }

    fn touch(&mut self) {
        self.dirty = true;
        self.rev += 1;
        self.welcome = false;
        let _ = std::fs::write(dirty_marker(), b"1");
    }

    pub fn undo(&mut self) {
        if let Some(p) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.project, p));
            self.touch();
            self.notify("Отменено", false);
        }
    }

    pub fn redo(&mut self) {
        if let Some(p) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.project, p));
            self.touch();
            self.notify("Повторено", false);
        }
    }

    pub fn notify(&mut self, msg: impl Into<String>, error: bool) {
        self.status = Some((msg.into(), error, now()));
    }

    pub fn report(&mut self) -> &Report {
        if self.report_cache.as_ref().is_none_or(|(r, _)| *r != self.rev) {
            let r = analyze(&self.project.school, &self.project.timetable);
            self.report_cache = Some((self.rev, r));
        }
        &self.report_cache.as_ref().unwrap().1
    }

    // ---------- файлы ----------

    fn set_project(&mut self, p: Project, path: Option<PathBuf>) {
        if let Some(old) = self.path.take() {
            if !self.read_only {
                fileio::release_lock(&old);
            }
        }
        self.read_only = false;
        self.loaded_mtime = path.as_deref().and_then(fileio::mtime);
        self.project = p;
        self.path = path.clone();
        self.dirty = false;
        self.rev += 1;
        self.undo.clear();
        self.redo.clear();
        self.welcome = false;
        self.lists_ui = Default::default();
        self.loads_ui = Default::default();
        self.tt_ui.reset_selection();
        self.subs_ui = Default::default();
        self.school_ui = Default::default();
        if let Some(p) = path {
            self.recent.retain(|x| *x != p);
            self.recent.insert(0, p);
            self.recent.truncate(8);
        }
        let _ = std::fs::remove_file(dirty_marker());
    }

    pub fn open_path(&mut self, p: PathBuf) {
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| Project::load_json(&t)) {
            Ok(pr) => {
                if let Some(info) = fileio::foreign_lock(&p) {
                    self.lock_conflict = Some((p, info, Box::new(pr)));
                    return;
                }
                self.finish_open(pr, p, false);
            }
            Err(e) => self.notify(format!("Не удалось открыть {}: {e}", p.display()), true),
        }
    }

    fn finish_open(&mut self, pr: Project, p: PathBuf, read_only: bool) {
        self.set_project(pr, Some(p.clone()));
        self.read_only = read_only;
        if read_only {
            self.notify(format!("Открыт только для чтения: {}", p.display()), false);
        } else {
            fileio::take_lock(&p);
            self.notify(format!("Открыт {}", p.display()), false);
        }
    }

    pub fn save(&mut self) -> bool {
        match self.path.clone() {
            Some(p) if !self.read_only => self.save_to(p),
            _ => self.save_as(),
        }
    }

    pub fn save_as(&mut self) -> bool {
        let name = format!("{}.skyurok.json", self.project.school.name.replace(['/', '\\', ':', '"', '«', '»'], ""));
        match fileio::pick_save("Проект SkyUrok", &["json"], &name) {
            Some(p) => self.save_to(p),
            None => false,
        }
    }

    fn save_to(&mut self, p: PathBuf) -> bool {
        // Файл изменили снаружи после открытия (другой компьютер, другая копия программы).
        let same = self.path.as_ref() == Some(&p);
        if same && self.overwrite_confirm.is_none() {
            if let (Some(was), Some(now)) = (self.loaded_mtime, fileio::mtime(&p)) {
                if now != was {
                    self.overwrite_confirm = Some(p);
                    return false;
                }
            }
        }
        self.overwrite_confirm = None;
        if let Err(e) = fileio::backup(&p) {
            self.notify(format!("Не удалось сделать резервную копию: {e}"), true);
        }
        match fileio::write(&p, &self.project.to_json()) {
            Ok(()) => {
                if !same {
                    if let Some(old) = self.path.take() {
                        if !self.read_only {
                            fileio::release_lock(&old);
                        }
                    }
                }
                self.read_only = false;
                fileio::take_lock(&p);
                self.loaded_mtime = fileio::mtime(&p);
                self.path = Some(p.clone());
                self.dirty = false;
                self.recent.retain(|x| *x != p);
                self.recent.insert(0, p.clone());
                let _ = std::fs::remove_file(dirty_marker());
                self.notify(format!("Сохранено: {}", p.display()), false);
                true
            }
            Err(e) => {
                self.notify(format!("Ошибка сохранения: {e}"), true);
                false
            }
        }
    }

    fn import_rtt(&mut self) {
        let Some(p) = fileio::pick_open("Файл расписания (.rtt)", &["rtt"]) else { return };
        let res = std::fs::read(&p).map_err(|e| e.to_string()).and_then(|b| rtt::read_lists(&b));
        match res {
            Ok(l) => {
                let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                let mut school = rtt::to_school(&l, &name);
                school.buildings[0].bells = default_bells(school.settings.periods_per_day);
                let msg = format!(
                    "Перенесено из файла .rtt: классов {}, кабинетов {}, предметов {}. Учителей и нагрузки пока нужно добавить вручную.",
                    school.classes.len(),
                    school.rooms.len(),
                    school.subjects.len()
                );
                self.set_project(Project::new(school), None);
                self.dirty = true;
                self.tab = Tab::Lists;
                self.notify(msg, false);
            }
            Err(e) => self.notify(format!("Не удалось прочитать .rtt: {e}"), true),
        }
    }

    fn run_pending(&mut self, a: Pending, ctx: &egui::Context) {
        match a {
            Pending::New => {
                self.set_project(Project::new(empty_school()), None);
                self.tab = Tab::School;
                if !self.onboarding_seen {
                    self.onboarding.open_at(0);
                }
                self.notify("Новая школа. Начните с раздела «Школа», затем заполните списки.", false);
            }
            Pending::Demo => {
                self.set_project(Project::new(demo::demo_school()), None);
                if !self.onboarding_seen {
                    self.onboarding.open_at(3);
                }
                self.tab = Tab::Timetable;
                self.notify("Открыта демо-школа. Нажмите «Составить», чтобы сгенерировать расписание.", false);
            }
            Pending::Open => {
                if let Some(p) = fileio::pick_open("Проект SkyUrok", &["json"]) {
                    self.open_path(p);
                }
            }
            Pending::OpenPath(p) => self.open_path(p),
            Pending::ImportRtt => self.import_rtt(),
            Pending::Restore => {
                let t = std::fs::read_to_string(autosave_path()).map_err(|e| e.to_string()).and_then(|t| Project::load_json(&t));
                match t {
                    Ok(p) => {
                        self.set_project(p, None);
                        self.dirty = true;
                        self.notify("Восстановлена несохранённая работа. Сохраните её в файл.", false);
                    }
                    Err(e) => self.notify(format!("Не удалось восстановить: {e}"), true),
                }
            }
            Pending::Quit => {
                self.allow_close = true;
                let _ = std::fs::remove_file(dirty_marker());
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn request(&mut self, a: Pending, ctx: &egui::Context) {
        if self.dirty {
            self.confirm = Some(a);
        } else {
            self.run_pending(a, ctx);
        }
    }

    // ---------- части окна ----------

    fn menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Файл", |ui| {
                if ui.button("Новая школа").clicked() {
                    self.request(Pending::New, &ctx);
                }
                if ui.button("Открыть…   Ctrl+O").clicked() {
                    self.request(Pending::Open, &ctx);
                }
                if !self.recent.is_empty() {
                    ui.menu_button("Недавние", |ui| {
                        for p in self.recent.clone() {
                            if ui.button(p.display().to_string()).clicked() {
                                self.request(Pending::OpenPath(p), &ctx);
                            }
                        }
                    });
                }
                ui.separator();
                if ui.button("Сохранить   Ctrl+S").clicked() {
                    self.save();
                }
                if ui.button("Сохранить как…").clicked() {
                    self.save_as();
                }
                if ui.add_enabled(self.path.is_some(), egui::Button::new("Резервные копии…")).clicked() {
                    self.backups_open = true;
                    ui.close();
                }
                ui.separator();
                if ui.button("Импорт из другой программы (.rtt)…").clicked() {
                    self.request(Pending::ImportRtt, &ctx);
                }
                if ui.button("Открыть демо-школу").clicked() {
                    self.request(Pending::Demo, &ctx);
                }
                ui.separator();
                if ui.button("Выход").clicked() {
                    self.request(Pending::Quit, &ctx);
                }
            });
            ui.menu_button("Правка", |ui| {
                if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Отменить   Ctrl+Z")).clicked() {
                    self.undo();
                }
                if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Повторить   Ctrl+Y")).clicked() {
                    self.redo();
                }
            });
            ui.menu_button("Вид", |ui| {
                for m in [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark] {
                    if ui.radio(self.theme_mode == m, format!("Тема: {}", m.label())).clicked() {
                        self.set_theme(&ctx, m);
                    }
                }
                ui.separator();
                ui.label(theme::bold("Масштаб интерфейса"));
                let cur = ctx.zoom_factor();
                for z in [0.9f32, 1.0, 1.25, 1.5, 1.75, 2.0] {
                    if ui.radio((cur - z).abs() < 0.01, format!("{:.0}%", z * 100.0)).clicked() {
                        ctx.set_zoom_factor(z);
                    }
                }
                ui.label(theme::small("Также Ctrl + / Ctrl − / Ctrl 0"));
            });
            ui.menu_button("Справка", |ui| {
                if ui.button("Обучение").clicked() {
                    self.onboarding.open_at(onboarding::first_incomplete(self));
                }
                if ui.button("Поиск и команды   Ctrl+K").clicked() {
                    self.palette.open();
                }
                if ui.button("О программе").clicked() {
                    self.about = true;
                }
            });
        });
    }

    fn nav(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.label(RichText::new("SkyUrok").size(22.0).family(crate::theme::bold_family()).color(theme::accent()));
        ui.label(theme::small(self.project.school.name.clone()));
        ui.add_space(10.0);
        let errors = if self.welcome { 0 } else { self.report().errors() };
        let unplaced = if self.welcome { 0 } else { self.report().stats.unplaced };
        let today = chrono::Local::now().date_naive();
        let absent_today = self.project.absences.iter().filter(|a| today >= a.from && today <= a.to).count();
        let s = &self.project.school;
        let items: [(Tab, &str, String); 6] = [
            (Tab::School, "Школа", format!("{} зд.", s.buildings.len())),
            (Tab::Lists, "Списки", format!("{} кл · {} уч", s.classes.len(), s.teachers.len())),
            (Tab::Loads, "Нагрузки", format!("{} ч", s.loads.iter().map(|l| l.hours_per_week as u32).sum::<u32>())),
            (
                Tab::Timetable,
                "Расписание",
                if errors > 0 {
                    format!("{errors} ошиб.")
                } else if unplaced > 0 {
                    format!("{unplaced} не расст.")
                } else {
                    format!("{} ур.", self.project.timetable.lessons.len())
                },
            ),
            (Tab::Subs, "Замены", if absent_today > 0 { format!("сегодня нет: {absent_today}") } else { format!("{}", self.project.replacements.len()) }),
            (Tab::Print, "Печать", String::new()),
        ];
        for (t, name, badge) in items {
            let sel = self.tab == t;
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), egui::Sense::click());
            let bg = if sel {
                theme::sel_bg()
            } else if resp.hovered() {
                theme::hover()
            } else {
                egui::Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, 6.0, bg);
            let font = if sel { egui::FontId::new(15.5, theme::bold_family()) } else { egui::FontId::proportional(15.5) };
            ui.painter().text(rect.left_center() + egui::vec2(12.0, 0.0), egui::Align2::LEFT_CENTER, name, font, theme::text());
            let warn = badge.contains("ошиб") || badge.contains("не расст") || badge.contains("сегодня");
            ui.painter().text(
                rect.right_center() - egui::vec2(10.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                &badge,
                egui::FontId::proportional(12.0),
                if warn { theme::bad() } else { theme::muted() },
            );
            if resp.clicked() {
                self.tab = t;
                self.welcome = false;
            }
            ui.add_space(2.0);
        }
        ui.add_space(12.0);
        onboarding::nav_card(self, ui);
        ui.add_space(8.0);
        if ui.add_sized([ui.available_width(), 30.0], egui::Button::new("Поиск и команды  Ctrl K")).clicked() {
            self.palette.open();
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let ctx = ui.ctx().clone();
            for m in [ThemeMode::Light, ThemeMode::Dark, ThemeMode::System] {
                let name = match m {
                    ThemeMode::Light => "Светлая",
                    ThemeMode::Dark => "Тёмная",
                    ThemeMode::System => "Авто",
                };
                if ui.selectable_label(self.theme_mode == m, name).on_hover_text(format!("Тема: {}", m.label())).clicked() {
                    self.set_theme(&ctx, m);
                }
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let file = self.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "не сохранено в файл".into());
            ui.label(theme::small(format!("{}{}", file, if self.dirty { " •" } else { "" })));
            ui.separator();
            if self.read_only {
                ui.label(RichText::new("только чтение").color(theme::warn()))
                    .on_hover_text("Файл открыт на другом компьютере. Чтобы вносить изменения, сохраните копию.");
                ui.separator();
            }
            if !self.welcome {
                let (errs, unpl) = {
                    let r = self.report();
                    (r.errors(), r.stats.unplaced)
                };
                let (txt, col) = if errs > 0 {
                    (format!("! Конфликтов: {errs}{}", if unpl > 0 { format!(" · не расставлено: {unpl} ч") } else { String::new() }), theme::bad())
                } else if unpl > 0 {
                    (format!("? Не расставлено: {unpl} ч"), theme::warn())
                } else {
                    ("✓ Конфликтов нет".to_string(), theme::ok())
                };
                if ui.link(RichText::new(txt).color(col)).clicked() {
                    self.tab = Tab::Timetable;
                }
                ui.separator();
            }
            if let Some(j) = self.tt_ui.job_status() {
                ui.spinner();
                ui.label(j);
                ui.separator();
            }
            if let Some((m, err, t)) = &self.status {
                if now() - t < 12.0 {
                    ui.label(RichText::new(m).color(if *err { theme::bad() } else { theme::ok() }));
                }
            }
        });
    }

    fn welcome_screen(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.label(RichText::new("SkyUrok").size(40.0).family(crate::theme::bold_family()).color(theme::accent()));
            ui.label(RichText::new("Составление школьного расписания и замен").size(18.0));
            ui.add_space(30.0);
            let w = 380.0;
            if ui.add_sized([w, 40.0], egui::Button::new("Демо-школа: 2 здания, 13 классов")).clicked() {
                self.run_pending(Pending::Demo, &ctx);
            }
            if ui.add_sized([w, 40.0], egui::Button::new("Новая школа")).clicked() {
                self.run_pending(Pending::New, &ctx);
            }
            if ui.add_sized([w, 40.0], egui::Button::new("Открыть файл проекта…")).clicked() {
                self.run_pending(Pending::Open, &ctx);
            }
            if ui.add_sized([w, 40.0], egui::Button::new("Перенести из другой программы (.rtt)…")).clicked() {
                self.run_pending(Pending::ImportRtt, &ctx);
            }
            ui.add_space(6.0);
            ui.label(theme::small("Пришли из другой программы расписания? Файл .rtt перенесёт классы, кабинеты и предметы."));
            ui.label(theme::small("Учителей и остальные списки удобно вставить из Excel кнопкой «Добавить списком» в разделе «Списки»."));
            if !self.recent.is_empty() {
                ui.add_space(20.0);
                ui.label(theme::bold("Недавние"));
                for p in self.recent.clone() {
                    if ui.link(p.display().to_string()).clicked() {
                        self.run_pending(Pending::OpenPath(p), &ctx);
                    }
                }
            }
        });
    }

    fn modals(&mut self, ctx: &egui::Context) {
        onboarding::dialog(self, ctx);
        palette::dialog(self, ctx);
        tab_timetable::result_dialog(self, ctx);
        tab_timetable::versions_dialog(self, ctx);
        tab_loads::import_dialogs(self, ctx);
        self.file_dialogs(ctx);
        tab_print::warning_dialog(self, ctx);
        if let Some(a) = self.confirm.clone() {
            let mut close = false;
            egui::Modal::new(egui::Id::unique("confirm")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.heading("Есть несохранённые изменения");
                ui.label("Сохранить их перед продолжением?");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Сохранить").clicked() && self.save() {
                        close = true;
                        self.run_pending(a.clone(), ctx);
                    }
                    if ui.button("Не сохранять").clicked() {
                        close = true;
                        self.dirty = false;
                        self.run_pending(a.clone(), ctx);
                    }
                    if ui.button("Отмена").clicked() {
                        close = true;
                    }
                });
            });
            if close {
                self.confirm = None;
            }
        }
        if self.restore_offer {
            egui::Modal::new(egui::Id::unique("restore")).show(ctx, |ui| {
                ui.set_width(400.0);
                ui.heading("Восстановить работу?");
                ui.label("Прошлый сеанс завершился без сохранения. Есть автосохранённая копия.");
                ui.horizontal(|ui| {
                    if ui.button("Восстановить").clicked() {
                        self.restore_offer = false;
                        self.run_pending(Pending::Restore, ctx);
                    }
                    if ui.button("Не нужно").clicked() {
                        self.restore_offer = false;
                        let _ = std::fs::remove_file(dirty_marker());
                    }
                });
            });
        }
        if self.about {
            let resp = egui::Modal::new(egui::Id::unique("about")).show(ctx, |ui| {
                ui.set_width(420.0);
                ui.heading("SkyUrok");
                ui.label(format!("Версия {} · © 2026 skysyndicatecorner", env!("CARGO_PKG_VERSION")));
                ui.label("Открытая программа для составления школьного расписания и замен.");
                ui.label("Windows · Linux · Android.");
                ui.label("Свободная программа: GNU AGPL-3.0 или более поздняя версия. Её можно использовать, изучать, изменять и распространять, но изменённые версии — в том числе работающие как онлайн-сервис — должны оставаться открытыми под той же лицензией.");
                ui.label(theme::small("Программа распространяется без каких-либо гарантий. Подробности — в файле LICENSE."));
                ui.label(theme::small("Шрифты Inter и DejaVu Sans — свободные лицензии (SIL OFL и Bitstream Vera)."));
                ui.add_space(6.0);
                if let Some(repo) = option_env!("SKYUROK_REPO").filter(|r| !r.is_empty()).or(Some(env!("CARGO_PKG_REPOSITORY"))).filter(|r| !r.is_empty()) {
                    if ui.link("Исходный код, обновления и сообщения об ошибках").clicked() {
                        let _ = webbrowser_open(repo);
                    }
                }
                ui.horizontal(|ui| {
                    ui.label(theme::small(format!("Папка данных: {}", fileio::data_dir().display())));
                    if ui.small_button("Открыть").clicked() {
                        let _ = fileio::open_external(&fileio::data_dir());
                    }
                });
                ui.add_space(6.0);
                if ui.button("Закрыть").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                    self.about = false;
                }
            });
            if resp.should_close() {
                self.about = false;
            }
        }
    }

    fn file_dialogs(&mut self, ctx: &egui::Context) {
        if let Some((path, info, _)) = &self.lock_conflict {
            let (path, info) = (path.clone(), info.clone());
            let mut choice: Option<u8> = None;
            egui::Modal::new(egui::Id::unique("lock_conflict")).show(ctx, |ui| {
                ui.set_width(520.0);
                ui.heading("Файл уже открыт на другом компьютере");
                ui.label(format!("{} открыл(а) его на компьютере «{}» в {}.", info.user, info.host, info.since));
                ui.label(theme::small(
                    "Если работать с одним файлом вдвоём, кто сохранит позже, тот сотрёт изменения другого. Откройте только для чтения или договоритесь, кто вносит правки.",
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(theme::bold("Только для чтения").color(theme::accent_text())).fill(theme::accent())).clicked() {
                        choice = Some(0);
                    }
                    if ui.button("Всё равно открыть").clicked() {
                        choice = Some(1);
                    }
                    if ui.button("Отмена").clicked() {
                        choice = Some(2);
                    }
                });
            });
            if let Some(c) = choice {
                let (_, _, pr) = self.lock_conflict.take().unwrap();
                match c {
                    0 => self.finish_open(*pr, path, true),
                    1 => self.finish_open(*pr, path, false),
                    _ => {}
                }
            }
        }
        if let Some(p) = self.overwrite_confirm.clone() {
            let mut choice: Option<u8> = None;
            egui::Modal::new(egui::Id::unique("overwrite_confirm")).show(ctx, |ui| {
                ui.set_width(520.0);
                ui.heading("Файл изменили после открытия");
                ui.label("Похоже, его сохранили с другого компьютера или из другой копии программы. Если перезаписать, те изменения пропадут.");
                ui.label(theme::small("Прежний вариант в любом случае попадёт в резервные копии."));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Сохранить как копию…").clicked() {
                        choice = Some(0);
                    }
                    if ui.button(RichText::new("Перезаписать").color(theme::bad())).clicked() {
                        choice = Some(1);
                    }
                    if ui.button("Отмена").clicked() {
                        choice = Some(2);
                    }
                });
            });
            match choice {
                Some(0) => {
                    self.overwrite_confirm = None;
                    self.save_as();
                }
                Some(1) => {
                    self.save_to(p);
                }
                Some(_) => self.overwrite_confirm = None,
                None => {}
            }
        }
        if self.backups_open {
            let list = self.path.as_deref().map(fileio::backups).unwrap_or_default();
            let mut restore: Option<PathBuf> = None;
            let mut close = false;
            let resp = egui::Modal::new(egui::Id::unique("backups")).show(ctx, |ui| {
                ui.set_width(520.0);
                ui.heading("Резервные копии");
                ui.label(theme::small(format!("Перед каждым сохранением прежний файл копируется сюда. Хранятся последние {}.", fileio::BACKUPS_KEEP)));
                ui.add_space(6.0);
                if list.is_empty() {
                    ui.label(theme::small("Копий пока нет: они появятся после первого сохранения."));
                }
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    egui::Grid::new("backups_grid").num_columns(3).striped(true).spacing([16.0, 6.0]).show(ui, |ui| {
                        for (p, label, size) in &list {
                            ui.label(label);
                            ui.label(theme::small(format!("{} КБ", size / 1024)));
                            if ui.small_button("Вернуть").on_hover_text("Загрузить эту копию; отменить можно через Ctrl+Z").clicked()
                            {
                                restore = Some(p.clone());
                            }
                            ui.end_row();
                        }
                    });
                });
                ui.add_space(8.0);
                if ui.button("Закрыть").clicked() {
                    close = true;
                }
            });
            if let Some(p) = restore {
                match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| Project::load_json(&t)) {
                    Ok(pr) => {
                        self.edit(|cur| *cur = pr);
                        self.tt_ui.map_cache = None;
                        self.notify("Резервная копия загружена. Сохраните, чтобы оставить её; Ctrl+Z — отменить", false);
                        close = true;
                    }
                    Err(e) => self.notify(format!("Не удалось прочитать копию: {e}"), true),
                }
            }
            if close || resp.should_close() {
                self.backups_open = false;
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let sc = |k| KeyboardShortcut::new(Modifiers::COMMAND, k);
        if ctx.input_mut(|i| i.consume_shortcut(&sc(Key::K))) && !self.welcome {
            self.palette.open();
        }
        let (z, y, s, o, shz) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&sc(Key::Z)),
                i.consume_shortcut(&sc(Key::Y)),
                i.consume_shortcut(&sc(Key::S)),
                i.consume_shortcut(&sc(Key::O)),
                i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)),
            )
        });
        if z && !ctx.egui_wants_keyboard_input() {
            self.undo();
        }
        if (y || shz) && !ctx.egui_wants_keyboard_input() {
            self.redo();
        }
        if s {
            self.save();
        }
        if o {
            self.request(Pending::Open, ctx);
        }
    }

    fn autosave(&mut self) {
        let t = now();
        if self.dirty && t - self.last_autosave > 60.0 {
            self.last_autosave = t;
            let _ = fileio::write(&autosave_path(), &self.project.to_json());
        }
    }
}

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

impl eframe::App for SkyApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        theme::sync(&ctx);
        self.zoom = ctx.zoom_factor();
        self.shortcuts(&ctx);
        tab_timetable::poll(self, &ctx);
        self.autosave();
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Pending::Quit);
        }
        let title = format!(
            "SkyUrok — {}{}",
            self.path.as_ref().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| self.project.school.name.clone()),
            if self.dirty { " •" } else { "" }
        );
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        if !self.welcome {
            egui::Panel::left("nav").exact_size(210.0).resizable(false).show(ui, |ui| self.nav(ui));
        }
        egui::CentralPanel::default().show(ui, |ui| {
            if self.welcome {
                self.welcome_screen(ui);
                return;
            }
            if self.locked() {
                egui::Frame::new().fill(theme::sel_bg()).corner_radius(8.0).inner_margin(10.0).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.spinner();
                        ui.label(theme::bold("Идёт составление расписания.").color(theme::sel_text()));
                        ui.label(
                            RichText::new("Данные заблокированы до окончания: результат считается по ним. Расписание можно смотреть.").color(theme::sel_text()),
                        );
                        if ui.button("Остановить").clicked() {
                            self.tt_ui.stop();
                        }
                    });
                });
                ui.add_space(6.0);
            }
            let lock = self.locked() && self.tab != Tab::Timetable && self.tab != Tab::Print;
            ui.add_enabled_ui(!lock, |ui| match self.tab {
                Tab::School => tab_school::show(self, ui),
                Tab::Lists => tab_lists::show(self, ui),
                Tab::Loads => tab_loads::show(self, ui),
                Tab::Timetable => tab_timetable::show(self, ui),
                Tab::Subs => tab_subs::show(self, ui),
                Tab::Print => tab_print::show(self, ui),
            });
        });
        self.modals(&ctx);
        let _ = widgets::heading; // используется во вкладках
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let r: Vec<String> = self.recent.iter().map(|p| p.display().to_string()).collect();
        storage.set_string("recent", serde_json::to_string(&r).unwrap_or_default());
        storage.set_string("zoom", self.zoom.to_string());
        storage.set_string("theme", serde_json::to_string(&self.theme_mode).unwrap_or_default());
        storage.set_string("onboarding_seen", if self.onboarding_seen { "1".into() } else { "0".into() });
    }
}

#[cfg(not(target_os = "android"))]
fn webbrowser_open(url: &str) -> Result<(), String> {
    webbrowser::open(url).map_err(|e| e.to_string())
}

#[cfg(target_os = "android")]
fn webbrowser_open(_url: &str) -> Result<(), String> {
    Err("недоступно на Android".into())
}

impl Drop for SkyApp {
    fn drop(&mut self) {
        if let Some(p) = &self.path {
            if !self.read_only {
                fileio::release_lock(p);
            }
        }
    }
}
