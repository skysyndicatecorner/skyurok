// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::app::SkyApp;
use crate::{theme, widgets};
use eframe::egui::{self, CornerRadius, RichText, Sense, Stroke, StrokeKind, Vec2};
use skyurok_core::history::{self, RebaseReport};
use skyurok_core::manual::{self, Planner};
use skyurok_core::model::*;
use skyurok_core::solver::{self, SolveOptions, SolveProgress, SolveResult};
use skyurok_core::validate::{analyze, precheck, Report, Severity};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Class,
    Teacher,
    Room,
    School,
}

#[derive(Clone, Copy, Debug)]
enum DragItem {
    Lesson(Id),
    Unplaced(Id),
}

struct Job {
    handle: Option<JoinHandle<SolveResult>>,
    stop: Arc<AtomicBool>,
    progress: Arc<SolveProgress>,
    started: Instant,
    improve: bool,
    limit: u64,
}

type FeasMap = HashMap<Slot, Result<Vec<Id>, Vec<String>>>;

pub struct PendingResult {
    res: SolveResult,
    before: Report,
    after: Report,
    moved: usize,
    improve: bool,
    rebase: RebaseReport,
}

pub struct State {
    view: View,
    class: Option<Id>,
    teacher: Option<Id>,
    room: Option<Id>,
    school_day: u8,
    selected: Option<Id>,
    seconds: u64,
    seed: u64,
    job: Option<Job>,
    pub(crate) map_cache: Option<((u64, Id, Option<Id>), FeasMap)>,
    pub pending: Option<PendingResult>,
    pub versions_open: bool,
    version_name: String,
    cursor: Option<Slot>,
    kb_move: Option<Id>,
    kb_swap: Option<Id>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            view: View::Class,
            class: None,
            teacher: None,
            room: None,
            school_day: 0,
            selected: None,
            seconds: 20,
            seed: 1,
            job: None,
            map_cache: None,
            pending: None,
            versions_open: false,
            version_name: String::new(),
            cursor: None,
            kb_move: None,
            kb_swap: None,
        }
    }
}

impl State {
    pub fn reset_selection(&mut self) {
        self.class = None;
        self.teacher = None;
        self.room = None;
        self.selected = None;
        self.map_cache = None;
    }

    pub fn job_running(&self) -> bool {
        self.job.is_some()
    }

    pub fn stop(&mut self) {
        if let Some(j) = &self.job {
            j.stop.store(true, Ordering::Relaxed);
        }
    }

    pub fn show_class(&mut self, id: Id) {
        self.view = View::Class;
        self.class = Some(id);
    }
    pub fn show_teacher(&mut self, id: Id) {
        self.view = View::Teacher;
        self.teacher = Some(id);
    }
    pub fn show_room(&mut self, id: Id) {
        self.view = View::Room;
        self.room = Some(id);
    }

    pub fn job_status(&self) -> Option<String> {
        let j = self.job.as_ref()?;
        let p = &j.progress;
        let phase = match p.phase.load(Ordering::Relaxed) {
            0 => "подготовка",
            1 => "расстановка",
            _ => "улучшение",
        };
        Some(format!(
            "{}: {} · {:.0} из {} с · оценка {} · не расставлено {}",
            if j.improve { "Улучшение" } else { "Составление" },
            phase,
            j.started.elapsed().as_secs_f32(),
            j.limit,
            p.best_score.load(Ordering::Relaxed),
            p.unplaced.load(Ordering::Relaxed)
        ))
    }
}

/// Вызывается каждый кадр: забирает результат фоновой генерации.
pub fn poll(app: &mut SkyApp, ctx: &egui::Context) {
    let finished = app.tt_ui.job.as_ref().is_some_and(|j| j.handle.as_ref().is_some_and(|h| h.is_finished()));
    if finished {
        let mut j = app.tt_ui.job.take().unwrap();
        match j.handle.take().unwrap().join() {
            Ok(res) => {
                if app.project.timetable.lessons.is_empty() {
                    let msg = format!("Готово: {} уроков, не расставлено {} ({:.1} с)", res.timetable.lessons.len(), res.unplaced, res.elapsed.as_secs_f32());
                    app.edit(|p| p.timetable = res.timetable);
                    app.notify(msg, res.unplaced > 0);
                } else {
                    let before = analyze(&app.project.school, &app.project.timetable);
                    let after = analyze(&app.project.school, &res.timetable);
                    let old: HashSet<(Id, Slot)> = app.project.timetable.lessons.iter().map(|l| (l.load, l.slot)).collect();
                    let moved = res.timetable.lessons.iter().filter(|l| !old.contains(&(l.load, l.slot))).count();
                    let (_, rebase) = history::rebase_replacements(&app.project, &res.timetable, today());
                    app.tt_ui.pending = Some(PendingResult { res, before, after, moved, improve: j.improve, rebase });
                }
                app.tt_ui.map_cache = None;
            }
            Err(_) => app.notify("Генерация завершилась с ошибкой", true),
        }
    } else if app.tt_ui.job.is_some() {
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

pub fn start(app: &mut SkyApp, improve: bool) {
    if app.tt_ui.job.is_some() {
        return;
    }
    if app.project.school.loads.is_empty() {
        app.notify("Нет нагрузок — нечего расставлять", true);
        return;
    }
    start_job(app, improve);
}

/// Окно «было / стало» после генерации.
pub fn result_dialog(app: &mut SkyApp, ctx: &egui::Context) {
    let Some(pr) = &app.tt_ui.pending else { return };
    let mut decision: Option<bool> = None;
    egui::Modal::new(egui::Id::unique("solve_result")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.heading(if pr.improve { "Улучшенный вариант готов" } else { "Новый вариант готов" });
        ui.label(theme::small(format!(
            "Изменится уроков: {} из {} · считалось {:.1} с",
            pr.moved,
            pr.res.timetable.lessons.len(),
            pr.res.elapsed.as_secs_f32()
        )));
        ui.add_space(8.0);
        let b = &pr.before.stats;
        let a = &pr.after.stats;
        let rows: [(&str, i64, i64); 7] = [
            ("Конфликты", pr.before.errors() as i64, pr.after.errors() as i64),
            ("Не расставлено часов", b.unplaced, a.unplaced),
            ("Окна у классов", b.class_windows, a.class_windows),
            ("Окна у учителей", b.teacher_single_windows + b.teacher_multi_windows, a.teacher_single_windows + a.teacher_multi_windows),
            ("Переходы между зданиями", b.teacher_building_changes, a.teacher_building_changes),
            ("Замечания СанПиН", b.hygiene_peak + b.hygiene_hardest_at_edge, a.hygiene_peak + a.hygiene_hardest_at_edge),
            ("Общая оценка (меньше — лучше)", pr.before.score, pr.after.score),
        ];
        egui::Grid::new("cmp").num_columns(4).striped(true).spacing([18.0, 6.0]).show(ui, |ui| {
            ui.label(theme::bold("Показатель"));
            ui.label(theme::bold("Сейчас"));
            ui.label(theme::bold("Новый"));
            ui.label("");
            ui.end_row();
            for (name, x, y) in rows {
                ui.label(name);
                ui.label(x.to_string());
                ui.label(theme::bold(y.to_string()));
                let (t, c) = if y < x {
                    ("лучше", theme::ok())
                } else if y > x {
                    ("хуже", theme::bad())
                } else {
                    ("так же", theme::muted())
                };
                ui.label(RichText::new(t).color(c));
                ui.end_row();
            }
        });
        let rb = &pr.rebase;
        if rb.kept + rb.shifted + rb.removed > 0 {
            ui.add_space(8.0);
            let warn = rb.touched() > 0;
            let frame = egui::Frame::new().corner_radius(8.0).inner_margin(10.0).fill(if warn { theme::warn_bg() } else { theme::surface2() });
            frame.show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(theme::bold("Назначенные замены на сегодня и дальше"));
                ui.label(format!("Останутся на своих уроках: {}", rb.kept));
                if rb.shifted > 0 {
                    ui.label(
                        RichText::new(format!("Урок переедет в тот же день — замена перенесётся с пометкой «пересмотреть»: {}", rb.shifted))
                            .color(theme::warn()),
                    );
                }
                if rb.removed > 0 {
                    let extra =
                        if rb.removed_confirmed > 0 { format!(", из них подтверждённых: {}", rb.removed_confirmed) } else { String::new() };
                    ui.label(RichText::new(format!("Урока в этот день больше не будет — замена удалится: {}{extra}", rb.removed)).color(theme::bad()));
                }
                ui.label(theme::small("Прошедшие замены остаются в истории и табеле. Текущее расписание сохранится как версия."));
            });
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(theme::bold("Применить новый").color(theme::accent_text())).fill(theme::accent())).clicked() {
                decision = Some(true);
            }
            if ui.button("Оставить текущий").clicked() {
                decision = Some(false);
            }
        });
        ui.label(theme::small("Применённый вариант можно отменить: Ctrl+Z."));
    });
    match decision {
        Some(true) => {
            let pr = app.tt_ui.pending.take().unwrap();
            let unpl = pr.res.unplaced;
            let mut rep = RebaseReport::default();
            let stamp = now_stamp();
            app.edit(|p| rep = history::apply_timetable(p, pr.res.timetable, today(), &stamp, &format!("Перед составлением {stamp}")));
            let msg = if rep.touched() > 0 {
                format!("Новый вариант применён. Замен перенесено с пометкой «пересмотреть»: {}, удалено: {}", rep.shifted, rep.removed)
            } else {
                "Новый вариант применён".to_string()
            };
            app.notify(msg, unpl > 0 || rep.touched() > 0);
        }
        Some(false) => {
            app.tt_ui.pending = None;
            app.notify("Оставлен текущий вариант", false);
        }
        None => {}
    }
}

fn start_job(app: &mut SkyApp, improve: bool) {
    let school = app.project.school.clone();
    let tt = app.project.timetable.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(SolveProgress::default());
    let opts = SolveOptions {
        seed: app.tt_ui.seed,
        max_iterations: u64::MAX / 2,
        time_limit: Duration::from_secs(app.tt_ui.seconds),
        keep_pinned_from: Some(tt.clone()),
        start_from: if improve { Some(tt) } else { None },
        stop: Some(stop.clone()),
        progress: Some(progress.clone()),
    };
    let handle = std::thread::spawn(move || solver::solve_parallel(&school, &opts, solver::default_threads()));
    app.tt_ui.job = Some(Job { handle: Some(handle), stop, progress, started: Instant::now(), improve, limit: app.tt_ui.seconds });
}

fn subj_short(ix: &Index, id: Id) -> String {
    ix.subject(id).map(|s| if s.short.is_empty() { s.name.clone() } else { s.short.clone() }).unwrap_or_default()
}

fn lesson_lines(ix: &Index, l: &Lesson, view: View, entity: Option<Id>) -> String {
    let Some(load) = ix.load(l.load) else { return "?".into() };
    let mut out = vec![];
    for (gi, g) in load.groups.iter().enumerate() {
        if view == View::Teacher && Some(g.teacher) != entity {
            continue;
        }
        if view == View::Room && l.rooms.get(gi).copied() != entity {
            continue;
        }
        let room = l.rooms.get(gi).and_then(|r| ix.room(*r)).map(|r| r.name.clone()).unwrap_or_else(|| "—".into());
        let grp = if g.label.is_empty() { String::new() } else { format!(" ({})", g.label) };
        out.push(match view {
            View::Class | View::School => format!("{}{}\n{} · {}", subj_short(ix, g.subject), grp, ix.teacher_name(g.teacher), room),
            View::Teacher => format!("{} {}{}\nкаб. {}", ix.classes_label(&load.classes), subj_short(ix, g.subject), grp, room),
            View::Room => format!("{} {}\n{}", ix.classes_label(&load.classes), subj_short(ix, g.subject), ix.teacher_name(g.teacher)),
        });
    }
    out.join("\n")
}

fn lesson_in_view(ix: &Index, l: &Lesson, view: View, entity: Id) -> bool {
    let Some(load) = ix.load(l.load) else { return false };
    match view {
        View::Class | View::School => load.classes.contains(&entity),
        View::Teacher => load.groups.iter().any(|g| g.teacher == entity),
        View::Room => l.rooms.contains(&entity),
    }
}

fn feas_map(app: &mut SkyApp, load: Id, moving: Option<Id>) -> FeasMap {
    let key = (app.rev, load, moving);
    if let Some((k, m)) = &app.tt_ui.map_cache {
        if *k == key {
            return m.clone();
        }
    }
    let m = Planner::new(&app.project.school, &app.project.timetable).map(load, moving);
    app.tt_ui.map_cache = Some((key, m.clone()));
    m
}

pub fn show(app: &mut SkyApp, ui: &mut egui::Ui) {
    toolbar(app, ui);
    ui.separator();
    egui::Panel::right("tt_side").default_size(330.0).resizable(true).show(ui, |ui| side(app, ui));
    egui::CentralPanel::default().show(ui, |ui| {
        if app.project.school.classes.is_empty() {
            ui.label(theme::small("Сначала добавьте классы, учителей и нагрузки."));
            return;
        }
        if app.tt_ui.view == View::School {
            school_grid(app, ui);
        } else {
            week_grid(app, ui);
        }
    });
}

fn toolbar(app: &mut SkyApp, ui: &mut egui::Ui) {
    let s = app.project.school.clone();
    ui.horizontal_wrapped(|ui| {
        widgets::heading(ui, "Расписание");
        ui.add_space(10.0);
        for (v, n) in [(View::Class, "Класс"), (View::Teacher, "Учитель"), (View::Room, "Кабинет"), (View::School, "Вся школа")] {
            if ui.selectable_label(app.tt_ui.view == v, n).clicked() {
                app.tt_ui.view = v;
            }
        }
        ui.separator();
        let (items, cur): (Vec<(Id, String)>, &mut Option<Id>) = match app.tt_ui.view {
            View::Class | View::School => (s.classes.iter().map(|c| (c.id, c.name.clone())).collect(), &mut app.tt_ui.class),
            View::Teacher => (s.teachers.iter().map(|t| (t.id, t.short_name())).collect(), &mut app.tt_ui.teacher),
            View::Room => (s.rooms.iter().map(|r| (r.id, Index::new(&s).room_name(r.id))).collect(), &mut app.tt_ui.room),
        };
        if cur.is_none_or(|c| !items.iter().any(|x| x.0 == c)) {
            *cur = items.first().map(|x| x.0);
        }
        if app.tt_ui.view == View::School {
            for d in 0..s.settings.days_per_week {
                if ui.selectable_label(app.tt_ui.school_day == d, DAY_NAMES[d as usize]).clicked() {
                    app.tt_ui.school_day = d;
                }
            }
        } else if let Some(mut c) = *cur {
            let pos = items.iter().position(|x| x.0 == c).unwrap_or(0);
            if ui.small_button("‹").clicked() && pos > 0 {
                c = items[pos - 1].0;
            }
            widgets::combo_req(ui, "tt_entity", &mut c, &items, 190.0);
            if ui.small_button("›").clicked() && pos + 1 < items.len() {
                c = items[pos + 1].0;
            }
            *cur = Some(c);
        }
        ui.separator();
        let running = app.tt_ui.job.is_some();
        if running {
            if ui.button(theme::bold("Остановить").color(theme::bad())).clicked() {
                if let Some(j) = &app.tt_ui.job {
                    j.stop.store(true, Ordering::Relaxed);
                }
            }
        } else {
            if ui
                .add(egui::Button::new(theme::bold("Составить").color(theme::accent_text())).fill(theme::accent()))
                .on_hover_text("Составить заново. Закреплённые уроки останутся на местах. Результат можно сравнить с текущим.")
                .clicked()
            {
                start(app, false);
            }
            if ui.button("Улучшить").on_hover_text("Продолжить от текущего расписания: меньше окон, лучше по требованиям").clicked()
            {
                start(app, true);
            }
        }
        ui.add(egui::DragValue::new(&mut app.tt_ui.seconds).range(3..=600).suffix(" с")).on_hover_text("Сколько времени искать");
        ui.add(egui::DragValue::new(&mut app.tt_ui.seed).range(1..=9999).prefix("вариант № ")).on_hover_text("Другой номер — другой вариант расписания");
        ui.separator();
        let entity = *match app.tt_ui.view {
            View::Teacher => &app.tt_ui.teacher,
            View::Room => &app.tt_ui.room,
            _ => &app.tt_ui.class,
        };
        let view = app.tt_ui.view;
        ui.menu_button("Ещё…", |ui| {
            if let Some(e) = entity.filter(|_| view != View::School) {
                if ui.button("Закрепить все уроки на экране").clicked() {
                    let ix_s = app.project.school.clone();
                    app.edit(|p| {
                        let ix = Index::new(&ix_s);
                        for l in p.timetable.lessons.iter_mut().filter(|l| lesson_in_view(&ix, l, view, e)) {
                            l.pinned = true;
                        }
                    });
                }
                if ui.button("Открепить все уроки на экране").clicked() {
                    let ix_s = app.project.school.clone();
                    app.edit(|p| {
                        let ix = Index::new(&ix_s);
                        for l in p.timetable.lessons.iter_mut().filter(|l| lesson_in_view(&ix, l, view, e)) {
                            l.pinned = false;
                        }
                    });
                }
                if ui.button("Убрать незакреплённые уроки с экрана").clicked() {
                    let ix_s = app.project.school.clone();
                    app.edit(|p| {
                        let ix = Index::new(&ix_s);
                        let ids: Vec<Id> = p.timetable.lessons.iter().filter(|l| !l.pinned && lesson_in_view(&ix, l, view, e)).map(|l| l.id).collect();
                        p.timetable.lessons.retain(|l| !ids.contains(&l.id));
                        p.replacements.retain(|r| !ids.contains(&r.lesson));
                    });
                }
                ui.separator();
            }
            if ui.button(format!("Версии расписания… ({})", app.project.versions.len())).clicked() {
                app.tt_ui.versions_open = true;
                ui.close();
            }
            ui.separator();
            if ui.button("Закрепить всё расписание").clicked() {
                app.edit(|p| p.timetable.lessons.iter_mut().for_each(|l| l.pinned = true));
            }
            if ui.button("Открепить всё").clicked() {
                app.edit(|p| p.timetable.lessons.iter_mut().for_each(|l| l.pinned = false));
            }
            if ui.button(RichText::new("Стереть всё расписание").color(theme::bad())).clicked() {
                app.edit(|p| {
                    p.timetable.lessons.clear();
                    p.replacements.clear();
                });
                app.notify("Расписание стёрто (Ctrl+Z — вернуть)", false);
            }
        });
    });
}

fn week_grid(app: &mut SkyApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let s = app.project.school.clone();
    let view = app.tt_ui.view;
    let entity = match view {
        View::Teacher => app.tt_ui.teacher,
        View::Room => app.tt_ui.room,
        _ => app.tt_ui.class,
    };
    let Some(entity) = entity else { return };
    let ix = Index::new(&s);
    let days = s.settings.days_per_week;
    let periods = s.settings.periods_per_day;
    let mut by_slot: HashMap<Slot, Vec<Lesson>> = HashMap::new();
    for l in &app.project.timetable.lessons {
        if lesson_in_view(&ix, l, view, entity) {
            by_slot.entry(l.slot).or_default().push(l.clone());
        }
    }
    // уроки с конфликтами
    let bad: HashSet<Id> = app.report().issues.iter().filter(|i| i.severity == Severity::Error).flat_map(|i| i.lessons.clone()).collect();
    let drag = egui::DragAndDrop::payload::<DragItem>(&ctx).map(|p| *p);
    let shift = ctx.input(|i| i.modifiers.shift);
    // ---- клавиатура: стрелки, M, S, P, Del, Enter, Esc ----
    let mut kb_drop: Option<(DragItem, Slot, Option<Id>)> = None;
    let mut kb_swap_do: Option<(Id, Id)> = None;
    let mut kb_act: Option<(Id, &'static str)> = None;
    let modal_open = app.palette_open() || app.tt_ui.pending.is_some();
    if !ctx.egui_wants_keyboard_input() && !modal_open && app.tt_ui.job.is_none() {
        use egui::{Key, Modifiers};
        let k = |key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key));
        let cur = app.tt_ui.cursor.get_or_insert(Slot::new(0, 0));
        let mut moved = false;
        if k(Key::ArrowLeft) && cur.day > 0 {
            cur.day -= 1;
            moved = true;
        }
        if k(Key::ArrowRight) && cur.day + 1 < days {
            cur.day += 1;
            moved = true;
        }
        if k(Key::ArrowUp) && cur.period > 0 {
            cur.period -= 1;
            moved = true;
        }
        if k(Key::ArrowDown) && cur.period + 1 < periods {
            cur.period += 1;
            moved = true;
        }
        let at = *cur;
        let here = by_slot.get(&at).and_then(|v| v.first()).map(|l| l.id);
        if moved && app.tt_ui.kb_move.is_none() && app.tt_ui.kb_swap.is_none() {
            app.tt_ui.selected = here;
        }
        if k(Key::M) {
            app.tt_ui.kb_move = here.or(app.tt_ui.selected);
            app.tt_ui.kb_swap = None;
        }
        if k(Key::S) {
            app.tt_ui.kb_swap = here.or(app.tt_ui.selected);
            app.tt_ui.kb_move = None;
        }
        if k(Key::P) {
            if let Some(h) = here {
                kb_act = Some((h, "pin"));
            }
        }
        if k(Key::Delete) || k(Key::Backspace) {
            if let Some(h) = here {
                kb_act = Some((h, "remove"));
            }
        }
        if k(Key::Escape) {
            app.tt_ui.kb_move = None;
            app.tt_ui.kb_swap = None;
        }
        if k(Key::Enter) {
            if let Some(m) = app.tt_ui.kb_move.take() {
                kb_drop = Some((DragItem::Lesson(m), at, here));
            } else if let Some(a) = app.tt_ui.kb_swap.take() {
                if let Some(b) = here.filter(|b| *b != a) {
                    kb_swap_do = Some((a, b));
                }
            } else {
                app.tt_ui.selected = here;
            }
        }
    }
    let kb_moving = app.tt_ui.kb_move;
    let map: Option<FeasMap> = match drag.or(kb_moving.map(DragItem::Lesson)) {
        Some(DragItem::Lesson(lid)) => app.project.timetable.lessons.iter().find(|l| l.id == lid).map(|l| l.load).map(|load| feas_map(app, load, Some(lid))),
        Some(DragItem::Unplaced(load)) => Some(feas_map(app, load, None)),
        None => None,
    };
    let bells: HashMap<u8, String> = s
        .classes
        .iter()
        .find(|c| Some(c.id) == app.tt_ui.class && view == View::Class)
        .and_then(|c| c.building)
        .or(s.buildings.first().map(|b| b.id))
        .and_then(|b| ix.building(b))
        .map(|b| b.bells.iter().map(|x| (x.period, format!("{}\n{}", x.start, x.end))).collect())
        .unwrap_or_default();

    let mut drop_action: Option<(DragItem, Slot, Option<Id>)> = None;
    let mut select: Option<Option<Id>> = None;
    let mut ctx_action: Option<(Id, &'static str)> = None;
    let mut hover_reason: Option<String> = None;

    let mode_text = if let Some(m) = app.tt_ui.kb_move {
        app.project.timetable.lessons.iter().find(|l| l.id == m).map(|l| {
            format!(
                "Перемещение: {} ({} {} ур.). Стрелки — выбрать ячейку, Enter — поставить, Esc — отмена.",
                lesson_lines(&ix, l, View::Class, None).lines().next().unwrap_or_default(),
                DAY_NAMES[l.slot.day as usize],
                l.slot.period + 1
            )
        })
    } else if let Some(m) = app.tt_ui.kb_swap {
        app.project.timetable.lessons.iter().find(|l| l.id == m).map(|l| {
            format!(
                "Обмен: {} — выберите второй урок и нажмите Enter. Esc — отмена.",
                lesson_lines(&ix, l, View::Class, None).lines().next().unwrap_or_default()
            )
        })
    } else {
        None
    };
    match mode_text {
        Some(t) => {
            egui::Frame::new().fill(theme::sel_bg()).corner_radius(6.0).inner_margin(8.0).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(RichText::new(t).color(theme::sel_text()));
            });
        }
        None => {
            ui.label(theme::small("Клавиатура: стрелки — по ячейкам · M — перенести · S — поменять местами · P — закрепить · Del — убрать. Урок можно перетащить мышью; на занятую ячейку — поменяет местами."));
        }
    }
    ui.add_space(4.0);
    egui::ScrollArea::both().id_salt("tt_grid").show(ui, |ui| {
        let head_w = 52.0;
        let avail = ui.available_width() - head_w - 8.0;
        let cell_w = (avail / days as f32).clamp(110.0, 260.0);
        let cell_h = 62.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(head_w + cell_w * days as f32, 24.0 + cell_h * periods as f32), Sense::hover());
        let painter = ui.painter_at(rect.expand(2.0));
        let font = egui::FontId::new(13.0, theme::bold_family());
        let small = egui::FontId::proportional(11.5);
        for d in 0..days {
            let r = egui::Rect::from_min_size(rect.min + Vec2::new(head_w + cell_w * d as f32, 0.0), Vec2::new(cell_w, 22.0));
            painter.text(r.center(), egui::Align2::CENTER_CENTER, DAY_NAMES_FULL[d as usize], font.clone(), theme::muted());
        }
        for p in 0..periods {
            let r = egui::Rect::from_min_size(rect.min + Vec2::new(0.0, 24.0 + cell_h * p as f32), Vec2::new(head_w - 4.0, cell_h - 4.0));
            painter.text(r.center_top() + Vec2::new(0.0, 6.0), egui::Align2::CENTER_TOP, format!("{}", p + 1), egui::FontId::proportional(16.0), theme::text());
            if let Some(b) = bells.get(&p) {
                painter.text(r.center_top() + Vec2::new(0.0, 26.0), egui::Align2::CENTER_TOP, b, egui::FontId::proportional(10.5), theme::muted());
            }
            for d in 0..days {
                let slot = Slot::new(d, p);
                let cell = egui::Rect::from_min_size(
                    rect.min + Vec2::new(head_w + cell_w * d as f32, 24.0 + cell_h * p as f32),
                    Vec2::new(cell_w - 4.0, cell_h - 4.0),
                );
                let id = egui::Id::unique(("ttcell", d, p));
                let resp = ui.interact(cell, id, Sense::click_and_drag());
                let lessons = by_slot.get(&slot).cloned().unwrap_or_default();
                let main = lessons.first();
                let pinned = main.is_some_and(|l| l.pinned);
                let selected = main.is_some_and(|l| Some(l.id) == app.tt_ui.selected);
                let availability = match view {
                    View::Class => ix.class(entity).map(|c| c.availability.get(slot)),
                    View::Teacher => ix.teacher(entity).map(|t| t.availability.get(slot)),
                    View::Room => ix.room(entity).map(|r| r.availability.get(slot)),
                    View::School => None,
                };
                let mut fill = if lessons.is_empty() {
                    if availability == Some(Avail::No) {
                        theme::closed_bg()
                    } else {
                        theme::surface()
                    }
                } else if pinned {
                    theme::pin_bg()
                } else {
                    theme::cell_bg()
                };
                if let Some(m) = &map {
                    match m.get(&slot) {
                        Some(Ok(_)) => fill = theme::ok_bg(),
                        Some(Err(why)) => {
                            if lessons.is_empty() {
                                fill = theme::bad_bg();
                            }
                            if resp.contains_pointer() {
                                hover_reason = Some(why.join("; "));
                            }
                        }
                        None => {}
                    }
                }
                if selected {
                    fill = theme::sel_bg();
                }
                painter.rect_filled(cell, CornerRadius::same(5), fill);
                let conflict = lessons.iter().any(|l| bad.contains(&l.id));
                // Подпись ячейки для программ экранного доступа (NVDA, Orca):
                // «Вторник, 3 урок: Алгебра, Иванова А.С., каб. 201. Конфликт».
                {
                    let mut label = format!("{}, {} урок: ", DAY_NAMES_FULL[d as usize], p + 1);
                    if lessons.is_empty() {
                        label.push_str("свободно");
                        if let Some(m) = &map {
                            match m.get(&slot) {
                                Some(Ok(_)) => label.push_str(", можно поставить"),
                                Some(Err(why)) => label.push_str(&format!(", нельзя: {}", why.join("; "))),
                                None => {}
                            }
                        }
                    } else {
                        let text = lessons.iter().map(|l| lesson_lines(&ix, l, view, Some(entity)).replace('\n', ", ")).collect::<Vec<_>>().join("; ");
                        label.push_str(&text);
                    }
                    if conflict {
                        label.push_str(". Конфликт");
                    }
                    if pinned {
                        label.push_str(". Закреплён");
                    }
                    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &label));
                }
                let stroke = if conflict {
                    Stroke::new(2.0, theme::bad())
                } else if resp.contains_pointer() && drag.is_some() {
                    Stroke::new(2.0, theme::accent())
                } else {
                    Stroke::new(1.0, theme::border())
                };
                painter.rect_stroke(cell, CornerRadius::same(5), stroke, StrokeKind::Inside);
                if app.tt_ui.cursor == Some(slot) && ui.ctx().memory(|m| m.focused().is_none()) {
                    painter.rect_stroke(cell.expand(2.0), CornerRadius::same(7), Stroke::new(2.0, theme::accent()), StrokeKind::Outside);
                }
                if lessons.is_empty() {
                    if let Some(m) = &map {
                        let (t, c) = match m.get(&slot) {
                            Some(Ok(_)) => ("✓ можно", theme::ok()),
                            Some(Err(_)) => ("× нельзя", theme::muted()),
                            None => ("", theme::muted()),
                        };
                        painter.text(cell.left_top() + Vec2::new(6.0, 6.0), egui::Align2::LEFT_TOP, t, small.clone(), c);
                    }
                }
                if conflict {
                    let b = egui::Rect::from_min_size(cell.right_top() + Vec2::new(-22.0, 4.0), Vec2::new(18.0, 18.0));
                    painter.rect_filled(b, CornerRadius::same(4), theme::bad());
                    painter.text(b.center(), egui::Align2::CENTER_CENTER, "!", font.clone(), theme::bad_bg());
                }
                if !lessons.is_empty() {
                    let cp = ui.painter_at(cell.shrink(2.0));
                    let text = lessons.iter().map(|l| lesson_lines(&ix, l, view, Some(entity))).collect::<Vec<_>>().join("\n");
                    let lines: Vec<&str> = text.lines().collect();
                    // строки уроков чередуются: заголовок (предмет) и подпись
                    let mut y = 5.0;
                    for (k, line) in lines.iter().enumerate() {
                        if y > cell.height() - 12.0 {
                            break;
                        }
                        let is_title = k % 2 == 0;
                        let (f, c) = if is_title { (font.clone(), theme::text()) } else { (small.clone(), theme::muted()) };
                        cp.text(cell.left_top() + Vec2::new(6.0, y), egui::Align2::LEFT_TOP, *line, f, c);
                        y += if is_title { 17.0 } else { 15.0 };
                    }
                    if pinned && !conflict {
                        cp.text(cell.right_top() + Vec2::new(-6.0, 4.0), egui::Align2::RIGHT_TOP, "◆", small.clone(), theme::accent());
                    }
                    if lessons.len() > 1 {
                        cp.text(
                            cell.right_bottom() + Vec2::new(-6.0, -4.0),
                            egui::Align2::RIGHT_BOTTOM,
                            format!("×{}", lessons.len()),
                            small.clone(),
                            theme::bad(),
                        );
                    }
                }
                if let (Some(l), true, false) = (main, resp.drag_started(), app.tt_ui.job.is_some()) {
                    resp.dnd_set_drag_payload(DragItem::Lesson(l.id));
                }
                if let Some(p) = resp.dnd_release_payload::<DragItem>() {
                    drop_action = Some((*p, slot, main.map(|l| l.id)));
                }
                if resp.clicked() {
                    select = Some(main.map(|l| l.id));
                    app.tt_ui.cursor = Some(slot);
                }
                if let Some(l) = main {
                    let lid = l.id;
                    let is_pinned = l.pinned;
                    resp.context_menu(|ui| {
                        if ui.button(if is_pinned { "Открепить" } else { "Закрепить" }).clicked() {
                            ctx_action = Some((lid, "pin"));
                            ui.close();
                        }
                        if ui.button("Перенести (M)").clicked() {
                            ctx_action = Some((lid, "move"));
                            ui.close();
                        }
                        if ui.button("Поменять местами с… (S)").clicked() {
                            ctx_action = Some((lid, "swap"));
                            ui.close();
                        }
                        if ui.button("Убрать из сетки (Del)").clicked() {
                            ctx_action = Some((lid, "remove"));
                            ui.close();
                        }
                        if ui.button("Показать учителя").clicked() {
                            ctx_action = Some((lid, "teacher"));
                            ui.close();
                        }
                        if ui.button("Показать класс").clicked() {
                            ctx_action = Some((lid, "class"));
                            ui.close();
                        }
                    });
                }
            }
        }
    });

    // плавающая подпись при перетаскивании
    if let Some(d) = drag {
        if let Some(pos) = ctx.pointer_hover_pos() {
            let txt = match d {
                DragItem::Lesson(lid) => app
                    .project
                    .timetable
                    .lessons
                    .iter()
                    .find(|l| l.id == lid)
                    .map(|l| lesson_lines(&ix, l, View::Class, None).lines().next().unwrap_or_default().to_string())
                    .unwrap_or_default(),
                DragItem::Unplaced(load) => ix
                    .load(load)
                    .map(|l| format!("{} {}", ix.classes_label(&l.classes), l.groups.iter().map(|g| subj_short(&ix, g.subject)).collect::<Vec<_>>().join("/")))
                    .unwrap_or_default(),
            };
            egui::Area::new(egui::Id::unique("drag_label")).order(egui::Order::Tooltip).fixed_pos(pos + Vec2::new(14.0, 10.0)).interactable(false).show(
                &ctx,
                |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.label(theme::bold(txt));
                        match &hover_reason {
                            Some(r) => {
                                ui.label(RichText::new(format!("✕ {r}")).color(theme::bad()));
                                ui.label(theme::small("Shift — поставить всё равно"));
                            }
                            None => {
                                ui.label(theme::small("зелёные ячейки — можно"));
                            }
                        }
                    });
                },
            );
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        }
    }

    if let Some(sel) = select {
        app.tt_ui.selected = sel;
    }
    if let Some((lid, act)) = ctx_action.or(kb_act) {
        match act {
            "move" => {
                app.tt_ui.kb_move = Some(lid);
                app.tt_ui.kb_swap = None;
            }
            "swap" => {
                app.tt_ui.kb_swap = Some(lid);
                app.tt_ui.kb_move = None;
            }
            "pin" => app.edit(|p| {
                if let Some(l) = p.timetable.lessons.iter_mut().find(|l| l.id == lid) {
                    l.pinned = !l.pinned;
                }
            }),
            "remove" => app.edit(|p| {
                p.timetable.lessons.retain(|l| l.id != lid);
                p.replacements.retain(|r| r.lesson != lid);
            }),
            "teacher" | "class" => {
                if let Some(load) = app.project.timetable.lessons.iter().find(|l| l.id == lid).and_then(|l| ix.load(l.load)) {
                    if act == "teacher" {
                        app.tt_ui.teacher = load.groups.first().map(|g| g.teacher);
                        app.tt_ui.view = View::Teacher;
                    } else {
                        app.tt_ui.class = load.classes.first().copied();
                        app.tt_ui.view = View::Class;
                    }
                    app.tt_ui.selected = Some(lid);
                }
            }
            _ => {}
        }
    }
    if let Some((item, slot, target)) = drop_action.or(kb_drop) {
        apply_drop(app, item, slot, target, shift);
    }
    if let Some((a, b)) = kb_swap_do {
        do_swap(app, a, b);
    }
}

fn do_swap(app: &mut SkyApp, a: Id, b: Id) {
    let school = app.project.school.clone();
    let mut tt = app.project.timetable.clone();
    match manual::swap_lessons(&school, &mut tt, a, b) {
        Ok(()) => {
            app.edit(|p| p.timetable = tt);
            app.tt_ui.selected = Some(a);
            app.notify("Уроки поменялись местами", false);
        }
        Err(why) => app.notify(format!("Поменять нельзя: {}", why.join("; ")), true),
    }
}

fn apply_drop(app: &mut SkyApp, item: DragItem, slot: Slot, target: Option<Id>, force: bool) {
    let school = app.project.school.clone();
    match item {
        DragItem::Lesson(lid) => {
            let Some(cur) = app.project.timetable.lessons.iter().find(|l| l.id == lid).cloned() else { return };
            if cur.slot == slot {
                return;
            }
            // на занятую ячейку — меняем местами
            if let Some(b) = target.filter(|b| *b != lid) {
                if !force {
                    do_swap(app, lid, b);
                    return;
                }
            }
            let mut tt = app.project.timetable.clone();
            match manual::move_lesson(&school, &mut tt, lid, slot) {
                Ok(()) => {
                    app.edit(|p| p.timetable = tt);
                    app.tt_ui.selected = Some(lid);
                }
                Err(why) if force => {
                    app.edit(|p| {
                        if let Some(l) = p.timetable.lessons.iter_mut().find(|l| l.id == lid) {
                            l.slot = slot;
                        }
                    });
                    app.notify(format!("Поставлено с нарушением: {}", why.join("; ")), true);
                }
                Err(why) => app.notify(format!("Нельзя: {}", why.join("; ")), true),
            }
        }
        DragItem::Unplaced(load) => {
            let mut tt = app.project.timetable.clone();
            match manual::place(&school, &mut tt, load, slot) {
                Ok(id) => {
                    app.edit(|p| p.timetable = tt);
                    app.tt_ui.selected = Some(id);
                }
                Err(why) if force => {
                    let rooms: Vec<Id> = school
                        .loads
                        .iter()
                        .find(|l| l.id == load)
                        .map(|l| {
                            l.groups
                                .iter()
                                .map(|g| {
                                    g.rooms
                                        .first()
                                        .copied()
                                        .or(school.teachers.iter().find(|t| t.id == g.teacher).and_then(|t| t.home_room))
                                        .or(school.rooms.first().map(|r| r.id))
                                        .unwrap_or(0)
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    app.edit(|p| {
                        manual::place_force(&mut p.timetable, load, slot, rooms);
                    });
                    app.notify(format!("Поставлено с нарушением: {}", why.join("; ")), true);
                }
                Err(why) => app.notify(format!("Нельзя: {}", why.join("; ")), true),
            }
        }
    }
}

fn school_grid(app: &mut SkyApp, ui: &mut egui::Ui) {
    let s = app.project.school.clone();
    let ix = Index::new(&s);
    let day = app.tt_ui.school_day.min(s.settings.days_per_week.saturating_sub(1));
    let periods = s.settings.periods_per_day;
    let mut by: HashMap<(Id, u8), Vec<&Lesson>> = HashMap::new();
    for l in app.project.timetable.lessons.iter().filter(|l| l.slot.day == day) {
        if let Some(load) = ix.load(l.load) {
            for c in &load.classes {
                by.entry((*c, l.slot.period)).or_default().push(l);
            }
        }
    }
    let mut goto = None;
    egui::ScrollArea::both().id_salt("school_grid").show(ui, |ui| {
        egui::Grid::new("sg").striped(true).spacing([4.0, 4.0]).show(ui, |ui| {
            ui.label(theme::bold(DAY_NAMES_FULL[day as usize]));
            for p in 0..periods {
                ui.label(theme::bold(format!("{}", p + 1)));
            }
            ui.end_row();
            for c in &s.classes {
                if ui.link(theme::bold(&c.name)).clicked() {
                    goto = Some(c.id);
                }
                for p in 0..periods {
                    let txt = by
                        .get(&(c.id, p))
                        .map(|v| {
                            v.iter()
                                .flat_map(|l| {
                                    ix.load(l.load)
                                        .map(|ld| {
                                            ld.groups
                                                .iter()
                                                .map(|g| {
                                                    format!(
                                                        "{} {}",
                                                        subj_short(&ix, g.subject),
                                                        ix.teacher(g.teacher).map(|t| t.last_name.clone()).unwrap_or_default()
                                                    )
                                                })
                                                .collect::<Vec<_>>()
                                        })
                                        .unwrap_or_default()
                                })
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default();
                    let w = egui::Label::new(RichText::new(txt).size(12.0)).selectable(false);
                    ui.add_sized([118.0, 34.0], w);
                }
                ui.end_row();
            }
        });
    });
    if let Some(c) = goto {
        app.tt_ui.class = Some(c);
        app.tt_ui.view = View::Class;
    }
}

fn side(app: &mut SkyApp, ui: &mut egui::Ui) {
    let s = app.project.school.clone();
    let ix = Index::new(&s);
    egui::ScrollArea::vertical().id_salt("tt_side_scroll").show(ui, |ui| {
        // выбранный урок
        if let Some(l) = app.tt_ui.selected.and_then(|id| app.project.timetable.lessons.iter().find(|l| l.id == id).cloned()) {
            widgets::card(ui, |ui| {
                let load = ix.load(l.load);
                ui.label(theme::bold(format!("{}, {} урок", DAY_NAMES_FULL[l.slot.day as usize], l.slot.period + 1)));
                if let Some(load) = load {
                    ui.label(format!("Класс: {}", ix.classes_label(&load.classes)));
                    for (gi, g) in load.groups.iter().enumerate() {
                        ui.label(format!(
                            "{}{} — {}, каб. {}",
                            ix.subject_name(g.subject),
                            if g.label.is_empty() { String::new() } else { format!(" ({})", g.label) },
                            ix.teacher_name(g.teacher),
                            l.rooms.get(gi).map(|r| ix.room_name(*r)).unwrap_or_else(|| "—".into())
                        ));
                    }
                }
                ui.horizontal(|ui| {
                    let lid = l.id;
                    if ui.button(if l.pinned { "Открепить" } else { "Закрепить" }).clicked() {
                        app.edit(|p| {
                            if let Some(x) = p.timetable.lessons.iter_mut().find(|x| x.id == lid) {
                                x.pinned = !x.pinned;
                            }
                        });
                    }
                    if ui.button("Убрать из сетки").clicked() {
                        app.edit(|p| {
                            p.timetable.lessons.retain(|x| x.id != lid);
                            p.replacements.retain(|r| r.lesson != lid);
                        });
                        app.tt_ui.selected = None;
                    }
                });
                ui.label(theme::small("Перетащите урок мышью на другую ячейку. Зелёные — можно, на красных подсказка покажет причину."));
            });
            ui.add_space(6.0);
        }

        // нерасставленные
        let entity = match app.tt_ui.view {
            View::Teacher => app.tt_ui.teacher,
            View::Room => None,
            _ => app.tt_ui.class,
        };
        let view = app.tt_ui.view;
        let un = manual::unplaced(&s, &app.project.timetable);
        let un: Vec<(Id, u8)> = un
            .into_iter()
            .filter(|(lid, _)| {
                ix.load(*lid).is_some_and(|l| match (view, entity) {
                    (View::Teacher, Some(t)) => l.groups.iter().any(|g| g.teacher == t),
                    (View::Class, Some(c)) => l.classes.contains(&c),
                    _ => true,
                })
            })
            .collect();
        widgets::card(ui, |ui| {
            ui.label(RichText::new(format!("Нерасставленные ({})", un.iter().map(|x| x.1 as u32).sum::<u32>())).family(crate::theme::bold_family()));
            if un.is_empty() {
                ui.label(theme::small("Все часы стоят в сетке."));
            } else {
                ui.label(theme::small("Перетащите в сетку."));
                // Почему не встало: то, что не помещается в принципе, — до любого составления.
                let why: Vec<String> = precheck(&app.project.school).into_iter().filter(|i| i.severity == Severity::Error).map(|i| i.message).collect();
                if !why.is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new("Не помещается в принципе:").color(theme::bad()));
                    for w in why.iter().take(5) {
                        ui.label(theme::small(format!("•  {w}")));
                    }
                }
            }
            for (lid, n) in un.iter().take(60) {
                let Some(l) = ix.load(*lid) else { continue };
                let text = format!(
                    "{} · {} · {}  ×{}",
                    ix.classes_label(&l.classes),
                    l.groups.iter().map(|g| subj_short(&ix, g.subject)).collect::<Vec<_>>().join("/"),
                    l.groups.iter().map(|g| ix.teacher(g.teacher).map(|t| t.last_name.clone()).unwrap_or_default()).collect::<Vec<_>>().join("/"),
                    n
                );
                ui.dnd_drag_source(egui::Id::unique(("unpl", *lid)), DragItem::Unplaced(*lid), |ui| {
                    egui::Frame::new().fill(theme::bad_bg()).corner_radius(4.0).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                        ui.label(text);
                    });
                });
            }
        });
        ui.add_space(6.0);

        // анализ
        let rep = app.report().clone();
        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::bold("Анализ"));
                let errs = rep.errors();
                ui.label(RichText::new(if errs == 0 { "конфликтов нет".to_string() } else { format!("ошибок: {errs}") }).color(if errs == 0 {
                    theme::ok()
                } else {
                    theme::bad()
                }));
            });
            ui.label(theme::small(format!("Оценка: {} (меньше — лучше)", rep.score)));
            for (k, v) in rep.stats.lines() {
                ui.label(format!("{k}: {v}"));
            }
            let mut jump = None;
            let issues: Vec<_> = rep.issues.iter().filter(|i| i.severity != Severity::Info).take(80).collect();
            if !issues.is_empty() {
                ui.separator();
            }
            for i in issues {
                let color = if i.severity == Severity::Error { theme::bad() } else { theme::warn() };
                let r = ui.add(
                    egui::Label::new(RichText::new(format!("{} {}", if i.severity == Severity::Error { "✕" } else { "!" }, i.message)).color(color).size(13.0))
                        .sense(Sense::click()),
                );
                if r.clicked() {
                    jump = i.lessons.first().copied();
                }
            }
            if let Some(lid) = jump {
                if let Some(load) = app.project.timetable.lessons.iter().find(|l| l.id == lid).and_then(|l| ix.load(l.load)) {
                    app.tt_ui.class = load.classes.first().copied();
                    app.tt_ui.view = View::Class;
                    app.tt_ui.selected = Some(lid);
                }
            }
        });
    });
}

fn today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

fn now_stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

/// Окно «Версии расписания»: сохранить текущее, вернуть или удалить версию.
pub fn versions_dialog(app: &mut SkyApp, ctx: &egui::Context) {
    if !app.tt_ui.versions_open {
        return;
    }
    let mut close = false;
    let mut restore: Option<Id> = None;
    let mut delete: Option<Id> = None;
    let mut save = false;
    let resp = egui::Modal::new(egui::Id::unique("versions")).show(ctx, |ui| {
        ui.set_width(600.0);
        ui.heading("Версии расписания");
        ui.label(theme::small(
            "Перед каждым составлением и восстановлением текущее расписание сохраняется само (последние 10). Свои версии храните под понятным именем — они не удаляются.",
        ));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut app.tt_ui.version_name).hint_text("Например: Утверждено 1 сентября").desired_width(320.0));
            if ui.add_enabled(!app.tt_ui.version_name.trim().is_empty(), egui::Button::new("Сохранить текущее")).clicked() {
                save = true;
            }
        });
        ui.add_space(6.0);
        if app.project.versions.is_empty() {
            ui.label(theme::small("Сохранённых версий пока нет."));
        }
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            egui::Grid::new("versions_grid").num_columns(4).striped(true).spacing([14.0, 6.0]).show(ui, |ui| {
                for v in app.project.versions.iter().rev() {
                    ui.label(if v.auto { RichText::new(&v.name).color(theme::muted()) } else { theme::bold(&v.name) });
                    ui.label(theme::small(&v.created));
                    ui.label(theme::small(format!("{} ур.", v.timetable.lessons.len())));
                    ui.horizontal(|ui| {
                        if ui.small_button("Вернуть").on_hover_text("Сделать это расписание текущим; текущее сохранится как версия").clicked() {
                            restore = Some(v.id);
                        }
                        if ui.small_button("✕").on_hover_text("Удалить версию").clicked() {
                            delete = Some(v.id);
                        }
                    });
                    ui.end_row();
                }
            });
        });
        ui.add_space(8.0);
        if ui.button("Закрыть").clicked() {
            close = true;
        }
    });
    if save {
        let name = app.tt_ui.version_name.trim().to_string();
        let stamp = now_stamp();
        app.edit(|p| {
            history::save_version(p, &name, &stamp, false);
        });
        app.tt_ui.version_name.clear();
        app.notify(format!("Версия «{name}» сохранена"), false);
    }
    if let Some(id) = restore {
        let stamp = now_stamp();
        let mut rep = None;
        app.edit(|p| rep = history::restore_version(p, id, today(), &stamp));
        app.tt_ui.map_cache = None;
        if let Some(r) = rep {
            app.notify(format!("Версия возвращена. Замен с пометкой «пересмотреть»: {}, удалено: {}", r.shifted, r.removed), r.touched() > 0);
        }
    }
    if let Some(id) = delete {
        app.edit(|p| p.versions.retain(|v| v.id != id));
    }
    if close || resp.should_close() {
        app.tt_ui.versions_open = false;
    }
}
