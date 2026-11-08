// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Поиск и команды (Ctrl+K): классы, учителя, кабинеты, разделы и действия.

use crate::app::{SkyApp, Tab};
use crate::tab_print::Format;
use crate::theme::{self, ThemeMode};
use crate::{tab_print, tab_subs, tab_timetable};
use eframe::egui::{self, Key};
use skyurok_core::model::Id;

#[derive(Default)]
pub struct State {
    open: bool,
    query: String,
    sel: usize,
    focus: bool,
}

impl State {
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.sel = 0;
        self.focus = true;
    }
}

#[derive(Clone)]
enum Cmd {
    Tab(Tab),
    Class(Id),
    Teacher(Id),
    Room(Id),
    Solve(bool),
    SubsToday,
    Export(Format),
    Theme(ThemeMode),
    Onboarding,
    Save,
    Undo,
}

fn items(app: &SkyApp) -> Vec<(String, &'static str, Cmd)> {
    let s = &app.project.school;
    let mut v: Vec<(String, &'static str, Cmd)> = vec![
        ("Составить расписание".into(), "действие", Cmd::Solve(false)),
        ("Улучшить расписание".into(), "действие", Cmd::Solve(true)),
        ("Подобрать замены на сегодня".into(), "действие", Cmd::SubsToday),
        ("Экспорт в Word".into(), "действие", Cmd::Export(Format::Word)),
        ("Экспорт в PDF".into(), "действие", Cmd::Export(Format::Pdf)),
        ("Сохранить".into(), "действие", Cmd::Save),
        ("Отменить последнее действие".into(), "действие", Cmd::Undo),
        ("Тёмная тема".into(), "вид", Cmd::Theme(ThemeMode::Dark)),
        ("Светлая тема".into(), "вид", Cmd::Theme(ThemeMode::Light)),
        ("Тема как в системе".into(), "вид", Cmd::Theme(ThemeMode::System)),
        ("Обучение".into(), "справка", Cmd::Onboarding),
        ("Школа: дни, уроки, здания, звонки".into(), "раздел", Cmd::Tab(Tab::School)),
        ("Списки: классы, учителя, предметы, кабинеты".into(), "раздел", Cmd::Tab(Tab::Lists)),
        ("Нагрузки".into(), "раздел", Cmd::Tab(Tab::Loads)),
        ("Расписание".into(), "раздел", Cmd::Tab(Tab::Timetable)),
        ("Замены".into(), "раздел", Cmd::Tab(Tab::Subs)),
        ("Печать и экспорт".into(), "раздел", Cmd::Tab(Tab::Print)),
    ];
    for c in &s.classes {
        v.push((format!("{} — расписание класса", c.name), "класс", Cmd::Class(c.id)));
    }
    for t in &s.teachers {
        v.push((format!("{} {} {}", t.last_name, t.first_name, t.middle_name).trim().to_string(), "учитель", Cmd::Teacher(t.id)));
    }
    for r in &s.rooms {
        v.push((format!("Кабинет {}", r.name), "кабинет", Cmd::Room(r.id)));
    }
    v
}

/// Перевод набранного в латинской раскладке в русскую (ghbdtn → привет).
fn layout_ru(q: &str) -> String {
    const EN: &str = "qwertyuiop[]asdfghjkl;'zxcvbnm,.`";
    const RU: &str = "йцукенгшщзхъфывапролджэячсмитьбюё";
    let ru: Vec<char> = RU.chars().collect();
    q.chars().map(|c| EN.chars().position(|e| e == c).map(|i| ru[i]).unwrap_or(c)).collect()
}

fn matches(text: &str, q: &str) -> bool {
    let t = text.to_lowercase();
    let q = q.to_lowercase();
    let hit = |q: &str| q.split_whitespace().all(|w| t.contains(w));
    hit(&q) || hit(&layout_ru(&q))
}

fn run(app: &mut SkyApp, ctx: &egui::Context, c: Cmd) {
    app.welcome = false;
    match c {
        Cmd::Tab(t) => app.tab = t,
        Cmd::Class(id) => {
            app.tt_ui.show_class(id);
            app.tab = Tab::Timetable;
        }
        Cmd::Teacher(id) => {
            app.tt_ui.show_teacher(id);
            app.tab = Tab::Timetable;
        }
        Cmd::Room(id) => {
            app.tt_ui.show_room(id);
            app.tab = Tab::Timetable;
        }
        Cmd::Solve(improve) => {
            app.tab = Tab::Timetable;
            tab_timetable::start(app, improve);
        }
        Cmd::SubsToday => {
            app.tab = Tab::Subs;
            tab_subs::auto_today(app);
        }
        Cmd::Export(f) => {
            app.tab = Tab::Print;
            tab_print::start_export(app, f);
        }
        Cmd::Theme(m) => app.set_theme(ctx, m),
        Cmd::Onboarding => app.onboarding.open_at(crate::onboarding::first_incomplete(app)),
        Cmd::Save => {
            app.save();
        }
        Cmd::Undo => app.undo(),
    }
}

pub fn dialog(app: &mut SkyApp, ctx: &egui::Context) {
    if !app.palette.open {
        return;
    }
    let all = items(app);
    let q = app.palette.query.clone();
    let found: Vec<&(String, &'static str, Cmd)> = all.iter().filter(|x| q.trim().is_empty() || matches(&x.0, &q)).take(12).collect();
    let (up, down, enter, esc) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, Key::Enter),
            i.consume_key(egui::Modifiers::NONE, Key::Escape),
        )
    });
    if down && app.palette.sel + 1 < found.len() {
        app.palette.sel += 1;
    }
    if up && app.palette.sel > 0 {
        app.palette.sel -= 1;
    }
    app.palette.sel = app.palette.sel.min(found.len().saturating_sub(1));
    let mut pick: Option<Cmd> = None;
    if enter {
        pick = found.get(app.palette.sel).map(|x| x.2.clone());
    }
    let mut close = esc;
    let resp = egui::Modal::new(egui::Id::unique("palette")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.set_min_height(480.0);
        let r = ui.add(
            egui::TextEdit::singleline(&mut app.palette.query)
                .hint_text("Класс, учитель, кабинет или команда…")
                .desired_width(f32::INFINITY)
                .font(egui::TextStyle::Heading),
        );
        if app.palette.focus {
            r.request_focus();
            app.palette.focus = false;
        }
        if r.changed() {
            app.palette.sel = 0;
        }
        ui.add_space(6.0);
        if found.is_empty() {
            ui.label(theme::small("Ничего не найдено"));
        }
        for (i, (name, kind, cmd)) in found.iter().enumerate() {
            let sel = i == app.palette.sel;
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::click());
            if sel || resp.hovered() {
                ui.painter().rect_filled(rect, 6.0, if sel { theme::sel_bg() } else { theme::hover() });
            }
            let fg = if sel { theme::sel_text() } else { theme::text() };
            ui.painter().text(rect.left_center() + egui::vec2(10.0, 0.0), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(15.0), fg);
            ui.painter().text(
                rect.right_center() - egui::vec2(10.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                *kind,
                egui::FontId::proportional(13.0),
                if sel { fg } else { theme::muted() },
            );
            if resp.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                app.palette.sel = i;
            }
            if resp.clicked() {
                pick = Some(cmd.clone());
            }
        }
        ui.add_space(4.0);
        ui.label(theme::small("↑ ↓ — выбор · Enter — открыть · Esc — закрыть"));
    });
    if resp.should_close() {
        close = true;
    }
    if let Some(c) = pick {
        app.palette.open = false;
        run(app, ctx, c);
    } else if close {
        app.palette.open = false;
    }
}
