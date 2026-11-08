// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::app::SkyApp;
use crate::{theme, widgets};
use eframe::egui::{self, RichText};
use skyurok_core::manual::Planner;
use skyurok_core::model::*;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Classes,
    Teachers,
    Subjects,
    Rooms,
}

#[derive(Clone, PartialEq)]
enum Draft {
    Class(Class),
    Teacher(Teacher),
    Subject(Subject),
    Room(Room),
}

#[derive(Default)]
pub struct State {
    pub kind: Kind,
    selected: Option<Id>,
    draft: Option<(Draft, bool)>,
    search: String,
    paste: Option<String>,
    confirm_delete: Option<(Kind, Id, String)>,
}

fn label(k: Kind) -> &'static str {
    match k {
        Kind::Classes => "Классы",
        Kind::Teachers => "Учителя",
        Kind::Subjects => "Предметы",
        Kind::Rooms => "Кабинеты",
    }
}

pub fn show(app: &mut SkyApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        widgets::heading(ui, "Списки");
        ui.add_space(16.0);
        for k in [Kind::Classes, Kind::Teachers, Kind::Subjects, Kind::Rooms] {
            if ui.selectable_label(app.lists_ui.kind == k, RichText::new(label(k)).size(15.5)).clicked() && app.lists_ui.kind != k {
                app.lists_ui.kind = k;
                app.lists_ui.selected = None;
                app.lists_ui.draft = None;
            }
        }
    });
    ui.separator();
    egui::Panel::left("list_left").default_size(430.0).resizable(true).show(ui, |ui| list(app, ui));
    egui::CentralPanel::default().show(ui, |ui| editor(app, ui));
    delete_dialog(app, ui.ctx());
    paste_dialog(app, ui.ctx());
}

fn hours_of_teacher(s: &School, id: Id) -> u32 {
    s.loads.iter().filter(|l| l.groups.iter().any(|g| g.teacher == id)).map(|l| l.hours_per_week as u32).sum()
}
fn hours_of_class(s: &School, id: Id) -> u32 {
    s.loads.iter().filter(|l| l.classes.contains(&id)).map(|l| l.hours_per_week as u32).sum()
}

fn list(app: &mut SkyApp, ui: &mut egui::Ui) {
    let k = app.lists_ui.kind;
    ui.horizontal(|ui| {
        if ui.button(theme::bold("+ Добавить")).clicked() {
            new_draft(app);
        }
        if ui.button("Добавить списком…").on_hover_text("Вставьте названия или ФИО из Word/Excel — по одному на строку").clicked()
        {
            app.lists_ui.paste = Some(String::new());
        }
        ui.add(egui::TextEdit::singleline(&mut app.lists_ui.search).hint_text("поиск").desired_width(140.0));
    });
    ui.add_space(4.0);
    let q = app.lists_ui.search.to_lowercase();
    let s = &app.project.school;
    let ix = Index::new(s);
    let rows: Vec<(Id, String, String)> = match k {
        Kind::Classes => s
            .classes
            .iter()
            .map(|c| {
                let b = c.building.and_then(|b| ix.building(b)).map(|b| b.name.clone()).unwrap_or_default();
                (c.id, c.name.clone(), format!("{} уч. · {} ч/нед · макс {} · {}", c.students, hours_of_class(s, c.id), c.max_per_day, b))
            })
            .collect(),
        Kind::Teachers => s
            .teachers
            .iter()
            .map(|t| {
                let subj: Vec<String> = t.subjects.iter().filter_map(|x| ix.subject(*x)).map(|x| x.short.clone()).collect();
                (
                    t.id,
                    format!("{} {} {}", t.last_name, t.first_name, t.middle_name).trim().to_string(),
                    format!("{} ч · {}", hours_of_teacher(s, t.id), subj.join(", ")),
                )
            })
            .collect(),
        Kind::Subjects => s
            .subjects
            .iter()
            .map(|x| (x.id, x.name.clone(), format!("{} · {} · трудность {}/{}/{}", x.short, x.area, x.difficulty[0], x.difficulty[1], x.difficulty[2])))
            .collect(),
        Kind::Rooms => s
            .rooms
            .iter()
            .map(|r| {
                let b = ix.building(r.building).map(|b| b.name.clone()).unwrap_or_default();
                (
                    r.id,
                    r.name.clone(),
                    format!(
                        "{} мест{} · {} {}",
                        r.capacity,
                        if r.parallel_groups > 1 { format!(" · {} гр.", r.parallel_groups) } else { String::new() },
                        b,
                        r.tags.join(", ")
                    ),
                )
            })
            .collect(),
    };
    let mut clicked = None;
    egui::ScrollArea::vertical().id_salt("listscroll").show(ui, |ui| {
        if rows.is_empty() {
            ui.label(theme::small("Список пуст. Нажмите «Добавить» или «Добавить списком»."));
        }
        for (id, name, info) in rows.iter().filter(|r| q.is_empty() || r.1.to_lowercase().contains(&q) || r.2.to_lowercase().contains(&q)) {
            let sel = app.lists_ui.selected == Some(*id);
            let resp = widgets::list_row(ui, sel, name, info).on_hover_text(info);
            if resp.clicked() {
                clicked = Some(*id);
            }
        }
    });
    if let Some(id) = clicked {
        select(app, id);
    }
}

fn select(app: &mut SkyApp, id: Id) {
    let s = &app.project.school;
    let d = match app.lists_ui.kind {
        Kind::Classes => s.classes.iter().find(|x| x.id == id).cloned().map(Draft::Class),
        Kind::Teachers => s.teachers.iter().find(|x| x.id == id).cloned().map(Draft::Teacher),
        Kind::Subjects => s.subjects.iter().find(|x| x.id == id).cloned().map(Draft::Subject),
        Kind::Rooms => s.rooms.iter().find(|x| x.id == id).cloned().map(Draft::Room),
    };
    app.lists_ui.selected = Some(id);
    app.lists_ui.draft = d.map(|d| (d, false));
}

fn new_draft(app: &mut SkyApp) {
    let s = &app.project.school;
    let d = match app.lists_ui.kind {
        Kind::Classes => Draft::Class(Class {
            id: widgets::next_id(&s.classes, |x| x.id),
            name: String::new(),
            grade: 5,
            students: 25,
            shift: 1,
            building: s.buildings.first().map(|b| b.id),
            max_per_day: 6,
            min_per_day: 4,
            availability: Default::default(),
        }),
        Kind::Teachers => Draft::Teacher(Teacher {
            id: widgets::next_id(&s.teachers, |x| x.id),
            last_name: String::new(),
            first_name: String::new(),
            middle_name: String::new(),
            subjects: vec![],
            home_room: None,
            availability: Default::default(),
            ignore_windows: false,
            methodical_days: 0,
            max_substitutions_per_week: Some(6),
            can_substitute: true,
            phone: String::new(),
            note: String::new(),
        }),
        Kind::Subjects => Draft::Subject(Subject {
            id: widgets::next_id(&s.subjects, |x| x.id),
            name: String::new(),
            short: String::new(),
            area: String::new(),
            difficulty: [0, 0, 0],
        }),
        Kind::Rooms => Draft::Room(Room {
            id: widgets::next_id(&s.rooms, |x| x.id),
            name: String::new(),
            building: s.buildings.first().map(|b| b.id).unwrap_or(1),
            capacity: 30,
            parallel_groups: 1,
            tags: vec![],
            availability: Default::default(),
        }),
    };
    app.lists_ui.selected = None;
    app.lists_ui.draft = Some((d, true));
}

fn editor(app: &mut SkyApp, ui: &mut egui::Ui) {
    let Some((mut d, is_new)) = app.lists_ui.draft.take() else {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| ui.label(theme::small("Выберите запись слева или добавьте новую.")));
        return;
    };
    let s = app.project.school.clone();
    let days = s.settings.days_per_week;
    let periods = s.settings.periods_per_day;
    let buildings: Vec<(Id, String)> = s.buildings.iter().map(|b| (b.id, b.name.clone())).collect();
    let rooms: Vec<(Id, String)> = s.rooms.iter().map(|r| (r.id, r.name.clone())).collect();
    let subjects: Vec<(Id, String)> = s.subjects.iter().map(|x| (x.id, x.name.clone())).collect();
    let mut save = false;
    let mut cancel = false;
    let mut delete = false;
    egui::ScrollArea::vertical().id_salt("editor").show(ui, |ui| {
        widgets::card(ui, |ui| {
            match &mut d {
                Draft::Class(c) => {
                    ui.label(theme::bold(if is_new { "Новый класс" } else { "Класс" }));
                    egui::Grid::new("class_form").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                        ui.label("Название");
                        let r = ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(120.0).hint_text("например, 5а"));
                        if r.changed() {
                            let g: String = c.name.chars().take_while(|ch| ch.is_ascii_digit()).collect();
                            if let Ok(g) = g.parse() {
                                c.grade = g;
                            }
                        }
                        ui.end_row();
                        ui.label("Параллель (год обучения)");
                        ui.add(egui::DragValue::new(&mut c.grade).range(0..=12));
                        ui.end_row();
                        ui.label("Учеников");
                        ui.add(egui::DragValue::new(&mut c.students).range(1..=60));
                        ui.end_row();
                        ui.label("Здание");
                        widgets::combo_opt(ui, "cl_b", &mut c.building, &buildings, "любое");
                        ui.end_row();
                        ui.label("Уроков в день: мин / макс");
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut c.min_per_day).range(0..=16));
                            ui.add(egui::DragValue::new(&mut c.max_per_day).range(1..=16));
                        });
                        ui.end_row();
                    });
                    ui.add_space(6.0);
                    ui.label(theme::bold("Когда класс может учиться"));
                    widgets::avail_grid(ui, "cl_av", &mut c.availability, days, periods, false);
                }
                Draft::Teacher(t) => {
                    ui.label(theme::bold(if is_new { "Новый учитель" } else { "Учитель" }));
                    egui::Grid::new("t_form").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                        ui.label("Фамилия");
                        ui.add(egui::TextEdit::singleline(&mut t.last_name).desired_width(220.0));
                        ui.end_row();
                        ui.label("Имя");
                        ui.add(egui::TextEdit::singleline(&mut t.first_name).desired_width(220.0));
                        ui.end_row();
                        ui.label("Отчество");
                        ui.add(egui::TextEdit::singleline(&mut t.middle_name).desired_width(220.0));
                        ui.end_row();
                        ui.label("Свой кабинет");
                        widgets::combo_opt(ui, "t_room", &mut t.home_room, &rooms, "нет");
                        ui.end_row();
                        ui.label("Методических дней");
                        ui.add(egui::DragValue::new(&mut t.methodical_days).range(0..=3));
                        ui.end_row();
                        ui.label("Телефон");
                        ui.add(egui::TextEdit::singleline(&mut t.phone).desired_width(220.0));
                        ui.end_row();
                        ui.label("Примечание");
                        ui.add(egui::TextEdit::singleline(&mut t.note).desired_width(320.0).hint_text("должность, пожелания"));
                        ui.end_row();
                    });
                    ui.checkbox(&mut t.ignore_windows, "Не считать окна (весь день в школе: завуч, директор)");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut t.can_substitute, "Выходит на замены");
                        let mut lim = t.max_substitutions_per_week.is_some();
                        if ui.checkbox(&mut lim, "не больше").changed() {
                            t.max_substitutions_per_week = if lim { Some(6) } else { None };
                        }
                        if let Some(m) = t.max_substitutions_per_week.as_mut() {
                            ui.add(egui::DragValue::new(m).range(0..=30));
                            ui.label("замен в неделю");
                        }
                    });
                    ui.add_space(6.0);
                    ui.label(theme::bold("Может вести предметы (важно для подбора замен)"));
                    widgets::multi_select(ui, "t_subj", &mut t.subjects, &subjects, 140.0);
                    ui.add_space(6.0);
                    ui.label(theme::bold("Рабочее время"));
                    widgets::avail_grid(ui, "t_av", &mut t.availability, days, periods, true);
                    if !is_new {
                        let ix = Index::new(&s);
                        let loads: Vec<String> = s
                            .loads
                            .iter()
                            .filter(|l| l.groups.iter().any(|g| g.teacher == t.id))
                            .map(|l| {
                                format!(
                                    "{} {} — {} ч",
                                    ix.classes_label(&l.classes),
                                    l.groups.iter().filter(|g| g.teacher == t.id).map(|g| ix.subject_name(g.subject)).collect::<Vec<_>>().join("/"),
                                    l.hours_per_week
                                )
                            })
                            .collect();
                        ui.add_space(6.0);
                        ui.label(RichText::new(format!("Нагрузка: {} ч в неделю", hours_of_teacher(&s, t.id))).family(crate::theme::bold_family()));
                        for l in loads {
                            ui.label(theme::small(l));
                        }
                    }
                }
                Draft::Subject(x) => {
                    ui.label(theme::bold(if is_new { "Новый предмет" } else { "Предмет" }));
                    let areas: Vec<String> = {
                        let mut a: Vec<String> = s.subjects.iter().map(|x| x.area.clone()).filter(|a| !a.is_empty()).collect();
                        a.sort();
                        a.dedup();
                        a
                    };
                    egui::Grid::new("s_form").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                        ui.label("Название");
                        ui.add(egui::TextEdit::singleline(&mut x.name).desired_width(260.0));
                        ui.end_row();
                        ui.label("Кратко (для сетки)");
                        ui.add(egui::TextEdit::singleline(&mut x.short).desired_width(100.0));
                        ui.end_row();
                        ui.label("Образовательная область");
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut x.area).desired_width(180.0));
                            egui::ComboBox::from_id_salt("area_pick").selected_text("…").width(40.0).show_ui(ui, |ui| {
                                for a in &areas {
                                    if ui.selectable_label(false, a).clicked() {
                                        x.area = a.clone();
                                    }
                                }
                            });
                        });
                        ui.end_row();
                        ui.label("Трудность: 1–4 / 5–9 / 10–11");
                        ui.horizontal(|ui| {
                            for v in x.difficulty.iter_mut() {
                                ui.add(egui::DragValue::new(v).range(0..=15));
                            }
                        });
                        ui.end_row();
                    });
                    ui.label(theme::small(
                        "Баллы — по шкалам трудности СанПиН (табл. 6.9–6.11 СанПиН 1.2.3685-21). Область помогает подобрать «смежного» учителя на замену.",
                    ));
                }
                Draft::Room(r) => {
                    ui.label(theme::bold(if is_new { "Новый кабинет" } else { "Кабинет" }));
                    let mut tags = r.tags.join(", ");
                    egui::Grid::new("r_form").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                        ui.label("Название");
                        ui.add(egui::TextEdit::singleline(&mut r.name).desired_width(160.0));
                        ui.end_row();
                        ui.label("Здание");
                        widgets::combo_req(ui, "r_b", &mut r.building, &buildings, 200.0);
                        ui.end_row();
                        ui.label("Мест");
                        ui.add(egui::DragValue::new(&mut r.capacity).range(1..=400));
                        ui.end_row();
                        ui.label("Групп одновременно");
                        ui.add(egui::DragValue::new(&mut r.parallel_groups).range(1..=6))
                            .on_hover_text("Спортзал, где одновременно занимаются 2 класса с разными учителями, — 2");
                        ui.end_row();
                        ui.label("Метки");
                        if ui.add(egui::TextEdit::singleline(&mut tags).hint_text("спортзал, химия…").desired_width(220.0)).changed() {
                            r.tags = tags.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
                        }
                        ui.end_row();
                    });
                    ui.label(theme::small("Кабинет с меткой не выдаётся «кому попало»: его получают только нагрузки, где он указан явно."));
                    ui.add_space(6.0);
                    ui.label(theme::bold("Когда кабинет доступен"));
                    widgets::avail_grid(ui, "r_av", &mut r.availability, days, periods, false);
                }
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button(theme::bold(if is_new { "Добавить" } else { "Сохранить" })).clicked() {
                    save = true;
                }
                if ui.button("Отмена").clicked() {
                    cancel = true;
                }
                if !is_new {
                    ui.add_space(30.0);
                    if ui.button(RichText::new("Удалить").color(theme::bad())).clicked() {
                        delete = true;
                    }
                }
            });
        });
    });

    if save {
        let err = match &d {
            Draft::Class(c) if c.name.trim().is_empty() => Some("Укажите название класса"),
            Draft::Teacher(t) if t.last_name.trim().is_empty() => Some("Укажите фамилию"),
            Draft::Subject(x) if x.name.trim().is_empty() => Some("Укажите название предмета"),
            Draft::Room(r) if r.name.trim().is_empty() => Some("Укажите название кабинета"),
            _ => None,
        };
        if let Some(e) = err {
            app.notify(e, true);
            app.lists_ui.draft = Some((d, is_new));
            return;
        }
        let mut d2 = d.clone();
        if let Draft::Subject(x) = &mut d2 {
            if x.short.trim().is_empty() {
                x.short = x.name.chars().take(5).collect();
            }
        }
        let id = upsert(app, d2);
        app.lists_ui.selected = Some(id);
        select(app, id);
        app.notify("Сохранено", false);
    } else if cancel {
        app.lists_ui.draft = None;
        app.lists_ui.selected = None;
    } else if delete {
        let (k, id, name) = match &d {
            Draft::Class(c) => (Kind::Classes, c.id, c.name.clone()),
            Draft::Teacher(t) => (Kind::Teachers, t.id, t.short_name()),
            Draft::Subject(x) => (Kind::Subjects, x.id, x.name.clone()),
            Draft::Room(r) => (Kind::Rooms, r.id, r.name.clone()),
        };
        app.lists_ui.confirm_delete = Some((k, id, name));
        app.lists_ui.draft = Some((d, is_new));
    } else {
        app.lists_ui.draft = Some((d, is_new));
    }
}

fn upsert(app: &mut SkyApp, d: Draft) -> Id {
    let mut out = 0;
    app.edit(|p| {
        let s = &mut p.school;
        macro_rules! up {
            ($vec:expr, $x:expr) => {{
                out = $x.id;
                match $vec.iter_mut().find(|e| e.id == $x.id) {
                    Some(e) => *e = $x,
                    None => $vec.push($x),
                }
            }};
        }
        match d {
            Draft::Class(c) => up!(s.classes, c),
            Draft::Teacher(t) => up!(s.teachers, t),
            Draft::Subject(x) => up!(s.subjects, x),
            Draft::Room(r) => up!(s.rooms, r),
        }
    });
    out
}

/// Удалить группу из нагрузки, поправив кабинеты уроков и замены.
fn remove_group(p: &mut Project, load_id: Id, gi: usize) {
    if let Some(l) = p.school.loads.iter_mut().find(|l| l.id == load_id) {
        if gi < l.groups.len() {
            l.groups.remove(gi);
        }
    }
    let lesson_ids: Vec<Id> = p.timetable.lessons.iter().filter(|l| l.load == load_id).map(|l| l.id).collect();
    for l in p.timetable.lessons.iter_mut().filter(|l| l.load == load_id) {
        if gi < l.rooms.len() {
            l.rooms.remove(gi);
        }
    }
    p.replacements.retain(|r| !(lesson_ids.contains(&r.lesson) && r.group == gi));
    for r in p.replacements.iter_mut().filter(|r| lesson_ids.contains(&r.lesson) && r.group > gi) {
        r.group -= 1;
    }
}

fn drop_empty_loads(p: &mut Project) {
    let dead: Vec<Id> = p.school.loads.iter().filter(|l| l.groups.is_empty() || l.classes.is_empty()).map(|l| l.id).collect();
    p.school.loads.retain(|l| !dead.contains(&l.id));
    let dead_lessons: Vec<Id> = p.timetable.lessons.iter().filter(|l| dead.contains(&l.load)).map(|l| l.id).collect();
    p.timetable.lessons.retain(|l| !dead.contains(&l.load));
    p.replacements.retain(|r| !dead_lessons.contains(&r.lesson));
}

fn delete_entity(p: &mut Project, k: Kind, id: Id) {
    match k {
        Kind::Classes => {
            p.school.classes.retain(|c| c.id != id);
            for l in &mut p.school.loads {
                l.classes.retain(|c| *c != id);
            }
        }
        Kind::Teachers => {
            p.school.teachers.retain(|t| t.id != id);
            let hits: Vec<(Id, usize)> =
                p.school.loads.iter().flat_map(|l| l.groups.iter().enumerate().filter(|(_, g)| g.teacher == id).map(move |(gi, _)| (l.id, gi))).collect();
            for (lid, gi) in hits.into_iter().rev() {
                remove_group(p, lid, gi);
            }
            p.absences.retain(|a| a.teacher != id);
            p.replacements.retain(|r| r.original_teacher != id && !matches!(r.kind, ReplacementKind::Substitute { teacher, .. } if teacher == id));
        }
        Kind::Subjects => {
            p.school.subjects.retain(|x| x.id != id);
            for t in &mut p.school.teachers {
                t.subjects.retain(|x| *x != id);
            }
            let hits: Vec<(Id, usize)> =
                p.school.loads.iter().flat_map(|l| l.groups.iter().enumerate().filter(|(_, g)| g.subject == id).map(move |(gi, _)| (l.id, gi))).collect();
            for (lid, gi) in hits.into_iter().rev() {
                remove_group(p, lid, gi);
            }
        }
        Kind::Rooms => {
            p.school.rooms.retain(|r| r.id != id);
            for l in &mut p.school.loads {
                for g in &mut l.groups {
                    g.rooms.retain(|r| *r != id);
                }
            }
            for t in &mut p.school.teachers {
                if t.home_room == Some(id) {
                    t.home_room = None;
                }
            }
            for r in &mut p.replacements {
                if r.room == Some(id) {
                    r.room = None;
                }
            }
            // перевыбрать кабинеты у уроков, где он стоял
            let affected: Vec<Id> = p.timetable.lessons.iter().filter(|l| l.rooms.contains(&id)).map(|l| l.id).collect();
            for lid in affected {
                let (load, slot) = {
                    let l = p.timetable.lessons.iter().find(|l| l.id == lid).unwrap();
                    (l.load, l.slot)
                };
                let res = Planner::new(&p.school, &p.timetable).check(load, slot, Some(lid));
                if let (Ok(rooms), Some(l)) = (res, p.timetable.lessons.iter_mut().find(|l| l.id == lid)) {
                    l.rooms = rooms;
                }
            }
        }
    }
    drop_empty_loads(p);
}

fn refs(s: &School, k: Kind, id: Id) -> usize {
    match k {
        Kind::Classes => s.loads.iter().filter(|l| l.classes.contains(&id)).count(),
        Kind::Teachers => s.loads.iter().filter(|l| l.groups.iter().any(|g| g.teacher == id)).count(),
        Kind::Subjects => s.loads.iter().filter(|l| l.groups.iter().any(|g| g.subject == id)).count(),
        Kind::Rooms => s.loads.iter().filter(|l| l.groups.iter().any(|g| g.rooms.contains(&id))).count(),
    }
}

fn delete_dialog(app: &mut SkyApp, ctx: &egui::Context) {
    let Some((k, id, name)) = app.lists_ui.confirm_delete.clone() else { return };
    let n = refs(&app.project.school, k, id);
    let mut close = false;
    egui::Modal::new(egui::Id::unique("del")).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.heading(format!("Удалить «{name}»?"));
        if n > 0 {
            ui.label(RichText::new(format!("Используется в нагрузках: {n}.")).color(theme::warn()));
            ui.label(match k {
                Kind::Rooms => "Кабинет уберётся из нагрузок, урокам подберутся другие кабинеты.",
                _ => "Связанные нагрузки (или группы в них), их уроки и замены тоже удалятся.",
            });
        }
        ui.label(theme::small("Действие можно отменить: Правка → Отменить (Ctrl+Z)."));
        ui.horizontal(|ui| {
            if ui.button(RichText::new("Удалить").color(theme::bad())).clicked() {
                app.edit(|p| delete_entity(p, k, id));
                app.lists_ui.draft = None;
                app.lists_ui.selected = None;
                app.notify(format!("Удалено: {name}"), false);
                close = true;
            }
            if ui.button("Отмена").clicked() {
                close = true;
            }
        });
    });
    if close {
        app.lists_ui.confirm_delete = None;
    }
}

fn paste_dialog(app: &mut SkyApp, ctx: &egui::Context) {
    let Some(mut text) = app.lists_ui.paste.take() else { return };
    let k = app.lists_ui.kind;
    let mut keep = true;
    egui::Modal::new(egui::Id::unique("paste")).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.heading(format!("Добавить списком: {}", label(k).to_lowercase()));
        ui.label(theme::small(match k {
            Kind::Classes => "По одному классу на строку: «5а» или «5а;27» (с числом учеников).",
            Kind::Teachers => "По одному учителю на строку: «Иванова Анна Сергеевна».",
            Kind::Subjects => "По одному предмету на строку: «Математика» или «Математика;Мат».",
            Kind::Rooms => "По одному кабинету на строку: «101» или «101;30» (с числом мест).",
        }));
        ui.add(egui::TextEdit::multiline(&mut text).desired_rows(12).desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            if ui.button("Добавить").clicked() {
                let lines: Vec<String> = text.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
                let n = lines.len();
                app.edit(|p| {
                    let s = &mut p.school;
                    let b = s.buildings.first().map(|b| b.id).unwrap_or(1);
                    for line in lines {
                        let parts: Vec<&str> = line.split([';', '\t']).map(|x| x.trim()).collect();
                        match k {
                            Kind::Classes => {
                                let id = widgets::next_id(&s.classes, |x| x.id);
                                let grade = parts[0].chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0);
                                s.classes.push(Class {
                                    id,
                                    name: parts[0].into(),
                                    grade,
                                    students: parts.get(1).and_then(|x| x.parse().ok()).unwrap_or(25),
                                    shift: 1,
                                    building: Some(b),
                                    max_per_day: if grade <= 4 { 5 } else { 7 },
                                    min_per_day: 4,
                                    availability: Default::default(),
                                });
                            }
                            Kind::Teachers => {
                                let id = widgets::next_id(&s.teachers, |x| x.id);
                                let w: Vec<&str> = parts[0].split_whitespace().collect();
                                s.teachers.push(Teacher {
                                    id,
                                    last_name: w.first().unwrap_or(&"").to_string(),
                                    first_name: w.get(1).unwrap_or(&"").to_string(),
                                    middle_name: w.get(2).unwrap_or(&"").to_string(),
                                    subjects: vec![],
                                    home_room: None,
                                    availability: Default::default(),
                                    ignore_windows: false,
                                    methodical_days: 0,
                                    max_substitutions_per_week: Some(6),
                                    can_substitute: true,
                                    phone: String::new(),
                                    note: String::new(),
                                });
                            }
                            Kind::Subjects => {
                                let id = widgets::next_id(&s.subjects, |x| x.id);
                                s.subjects.push(Subject {
                                    id,
                                    name: parts[0].into(),
                                    short: parts.get(1).map(|x| x.to_string()).unwrap_or_else(|| parts[0].chars().take(5).collect()),
                                    area: String::new(),
                                    difficulty: [0, 0, 0],
                                });
                            }
                            Kind::Rooms => {
                                let id = widgets::next_id(&s.rooms, |x| x.id);
                                s.rooms.push(Room {
                                    id,
                                    name: parts[0].into(),
                                    building: b,
                                    capacity: parts.get(1).and_then(|x| x.parse().ok()).unwrap_or(30),
                                    parallel_groups: 1,
                                    tags: vec![],
                                    availability: Default::default(),
                                });
                            }
                        }
                    }
                });
                app.notify(format!("Добавлено: {n}"), false);
                keep = false;
            }
            if ui.button("Отмена").clicked() {
                keep = false;
            }
        });
    });
    if keep {
        app.lists_ui.paste = Some(text);
    }
}
