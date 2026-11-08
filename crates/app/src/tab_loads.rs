// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::app::SkyApp;
use crate::{theme, widgets};
use eframe::egui::{self, RichText};
use skyurok_core::model::*;
use std::collections::HashMap;

#[derive(Default)]
pub struct State {
    filter_class: Option<Id>,
    filter_teacher: Option<Id>,
    selected: Option<Id>,
    draft: Option<(Load, bool)>,
    quick: Quick,
}

#[derive(Default)]
struct Quick {
    subject: Id,
    teacher: Id,
    hours: u8,
}

fn pairing_name(p: Pairing) -> &'static str {
    match p {
        Pairing::Any => "обычно (до 2 в день)",
        Pairing::Double => "только парами",
        Pairing::NoDouble => "не больше 1 в день",
    }
}
fn position_name(p: PositionReq) -> &'static str {
    match p {
        PositionReq::Any => "любой",
        PositionReq::First => "первым",
        PositionReq::Last => "последним",
        PositionReq::Edge => "первым или последним",
    }
}

pub fn show(app: &mut SkyApp, ui: &mut egui::Ui) {
    let s = app.project.school.clone();
    let ix = Index::new(&s);
    let classes: Vec<(Id, String)> = s.classes.iter().map(|c| (c.id, c.name.clone())).collect();
    let teachers: Vec<(Id, String)> = s.teachers.iter().map(|t| (t.id, t.short_name())).collect();
    let subjects: Vec<(Id, String)> = s.subjects.iter().map(|x| (x.id, x.name.clone())).collect();
    let rooms: Vec<(Id, String)> = s.rooms.iter().map(|r| (r.id, ix.room_name(r.id))).collect();
    let mut placed: HashMap<Id, usize> = HashMap::new();
    for l in &app.project.timetable.lessons {
        *placed.entry(l.load).or_default() += 1;
    }

    ui.horizontal(|ui| {
        widgets::heading(ui, "Нагрузки");
        ui.add_space(16.0);
        ui.label("Класс:");
        if widgets::combo_opt(ui, "lf_c", &mut app.loads_ui.filter_class, &classes, "все") {
            app.loads_ui.filter_teacher = None;
        }
        ui.label("Учитель:");
        if widgets::combo_opt(ui, "lf_t", &mut app.loads_ui.filter_teacher, &teachers, "все") {
            app.loads_ui.filter_class = None;
        }
        if ui.button(theme::bold("+ Нагрузка")).clicked() {
            let id = widgets::next_id(&s.loads, |l| l.id);
            let teacher = app.loads_ui.filter_teacher.or(s.teachers.first().map(|t| t.id)).unwrap_or(0);
            let subject =
                s.teachers.iter().find(|t| t.id == teacher).and_then(|t| t.subjects.first().copied()).or(s.subjects.first().map(|x| x.id)).unwrap_or(0);
            app.loads_ui.draft = Some((
                Load {
                    id,
                    classes: app.loads_ui.filter_class.into_iter().collect(),
                    hours_per_week: 2,
                    pairing: Pairing::Any,
                    position: PositionReq::Any,
                    groups: vec![Group { teacher, subject, rooms: vec![], label: String::new(), students: None }],
                    paid: false,
                },
                true,
            ));
            app.loads_ui.selected = None;
        }
    });
    // сводка по фильтру
    if let Some(c) = app.loads_ui.filter_class.and_then(|c| ix.class(c)) {
        let h: u32 = s.loads.iter().filter(|l| l.classes.contains(&c.id)).map(|l| l.hours_per_week as u32).sum();
        let cap = c.max_per_day as u32 * s.settings.days_per_week as u32;
        ui.label(
            RichText::new(format!(
                "{}: {} ч в неделю, вместимость сетки {} ч (макс {} в день × {} дн.)",
                c.name, h, cap, c.max_per_day, s.settings.days_per_week
            ))
            .color(if h > cap { theme::bad() } else { theme::muted() }),
        );
    }
    if let Some(t) = app.loads_ui.filter_teacher.and_then(|t| ix.teacher(t)) {
        let h: u32 = s.loads.iter().filter(|l| l.groups.iter().any(|g| g.teacher == t.id)).map(|l| l.hours_per_week as u32).sum();
        ui.label(theme::small(format!("{}: {} ч в неделю", t.short_name(), h)));
    }
    ui.separator();

    egui::Panel::right("load_editor").default_size(460.0).resizable(true).show(ui, |ui| editor(app, ui, &classes, &teachers, &subjects, &rooms));
    egui::CentralPanel::default().show(ui, |ui| {
        // быстрое добавление для выбранного класса
        if let Some(cid) = app.loads_ui.filter_class {
            widgets::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::bold("Быстро добавить:"));
                    if app.loads_ui.quick.hours == 0 {
                        app.loads_ui.quick.hours = 2;
                    }
                    widgets::combo_req(ui, "q_s", &mut app.loads_ui.quick.subject, &subjects, 170.0);
                    let qs = app.loads_ui.quick.subject;
                    let mut tlist: Vec<(Id, String)> = s.teachers.iter().filter(|t| t.subjects.contains(&qs)).map(|t| (t.id, t.short_name())).collect();
                    if tlist.is_empty() {
                        tlist = teachers.clone();
                    }
                    widgets::combo_req(ui, "q_t", &mut app.loads_ui.quick.teacher, &tlist, 160.0);
                    ui.add(egui::DragValue::new(&mut app.loads_ui.quick.hours).range(1..=12).suffix(" ч"));
                    let ok = ix.subject(qs).is_some() && ix.teacher(app.loads_ui.quick.teacher).is_some();
                    if ui.add_enabled(ok, egui::Button::new("Добавить")).clicked() {
                        let (t, h) = (app.loads_ui.quick.teacher, app.loads_ui.quick.hours);
                        app.edit(|p| {
                            let id = widgets::next_id(&p.school.loads, |l| l.id);
                            p.school.loads.push(Load {
                                id,
                                classes: vec![cid],
                                hours_per_week: h,
                                pairing: if h <= 2 { Pairing::NoDouble } else { Pairing::Any },
                                position: PositionReq::Any,
                                groups: vec![Group { teacher: t, subject: qs, rooms: vec![], label: String::new(), students: None }],
                                paid: false,
                            });
                            if let Some(tt) = p.school.teachers.iter_mut().find(|x| x.id == t) {
                                if !tt.subjects.contains(&qs) {
                                    tt.subjects.push(qs);
                                }
                            }
                        });
                        app.notify("Нагрузка добавлена", false);
                    }
                });
            });
        }
        let fc = app.loads_ui.filter_class;
        let ft = app.loads_ui.filter_teacher;
        let list: Vec<&Load> =
            s.loads.iter().filter(|l| fc.is_none_or(|c| l.classes.contains(&c)) && ft.is_none_or(|t| l.groups.iter().any(|g| g.teacher == t))).collect();
        let mut clicked = None;
        egui::ScrollArea::both().id_salt("loads_list").show(ui, |ui| {
            egui::Grid::new("loads_grid").striped(true).num_columns(6).spacing([14.0, 6.0]).show(ui, |ui| {
                for h in ["Классы", "Предмет", "Учитель", "Часов", "Расставлено", "Условия"] {
                    ui.label(theme::bold(h));
                }
                ui.end_row();
                for l in &list {
                    let sel = app.loads_ui.selected == Some(l.id);
                    if ui.selectable_label(sel, ix.classes_label(&l.classes)).clicked() {
                        clicked = Some(l.id);
                    }
                    ui.label(l.groups.iter().map(|g| ix.subject_name(g.subject)).collect::<Vec<_>>().join(" / "));
                    ui.label(l.groups.iter().map(|g| ix.teacher_name(g.teacher)).collect::<Vec<_>>().join(" / "));
                    ui.label(l.hours_per_week.to_string());
                    let p = placed.get(&l.id).copied().unwrap_or(0);
                    ui.label(RichText::new(format!("{p}/{}", l.hours_per_week)).color(if p < l.hours_per_week as usize { theme::bad() } else { theme::ok() }));
                    let mut cond = vec![];
                    if l.groups.len() > 1 {
                        cond.push(format!("{} гр.", l.groups.len()));
                    }
                    if l.classes.len() > 1 {
                        cond.push("поток".into());
                    }
                    if l.pairing != Pairing::Any {
                        cond.push(pairing_name(l.pairing).into());
                    }
                    if l.position != PositionReq::Any {
                        cond.push(position_name(l.position).into());
                    }
                    if l.paid {
                        cond.push("платно".into());
                    }
                    ui.label(theme::small(cond.join(", ")));
                    ui.end_row();
                }
            });
            if list.is_empty() {
                ui.label(theme::small("Нагрузок нет. Добавьте кнопкой «+ Нагрузка» или выберите класс для быстрого добавления."));
            }
        });
        if let Some(id) = clicked {
            app.loads_ui.selected = Some(id);
            app.loads_ui.draft = s.loads.iter().find(|l| l.id == id).cloned().map(|l| (l, false));
        }
    });
}

fn editor(app: &mut SkyApp, ui: &mut egui::Ui, classes: &[(Id, String)], teachers: &[(Id, String)], subjects: &[(Id, String)], rooms: &[(Id, String)]) {
    let Some((mut l, is_new)) = app.loads_ui.draft.take() else {
        ui.add_space(30.0);
        ui.label(theme::small("Выберите нагрузку в таблице или добавьте новую."));
        return;
    };
    let mut save = false;
    let mut cancel = false;
    let mut delete = false;
    let mut dup = false;
    egui::ScrollArea::vertical().id_salt("load_ed").show(ui, |ui| {
        ui.label(RichText::new(if is_new { "Новая нагрузка" } else { "Нагрузка" }).heading());
        ui.label(theme::bold("Классы (несколько — поток)"));
        widgets::multi_select(ui, "ld_cl", &mut l.classes, classes, 110.0);
        egui::Grid::new("ld_f").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Часов в неделю");
            ui.add(egui::DragValue::new(&mut l.hours_per_week).range(1..=20));
            ui.end_row();
            ui.label("Спаривание");
            egui::ComboBox::from_id_salt("ld_pair").selected_text(pairing_name(l.pairing)).width(200.0).show_ui(ui, |ui| {
                for p in [Pairing::Any, Pairing::Double, Pairing::NoDouble] {
                    ui.selectable_value(&mut l.pairing, p, pairing_name(p));
                }
            });
            ui.end_row();
            ui.label("Номер урока");
            egui::ComboBox::from_id_salt("ld_pos").selected_text(position_name(l.position)).width(200.0).show_ui(ui, |ui| {
                for p in [PositionReq::Any, PositionReq::First, PositionReq::Last, PositionReq::Edge] {
                    ui.selectable_value(&mut l.position, p, position_name(p));
                }
            });
            ui.end_row();
            ui.label("Платные уроки");
            ui.checkbox(&mut l.paid, "");
            ui.end_row();
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(theme::bold("Группы (занимаются одновременно)"));
            if ui.small_button("+ группа").clicked() {
                let g = l.groups.last().cloned().unwrap_or(Group { teacher: 0, subject: 0, rooms: vec![], label: String::new(), students: None });
                l.groups.push(Group { label: format!("{} гр.", l.groups.len() + 1), ..g });
                if l.groups.len() == 2 && l.groups[0].label.is_empty() {
                    l.groups[0].label = "1 гр.".into();
                }
            }
        });
        let mut remove = None;
        let n = l.groups.len();
        for (gi, g) in l.groups.iter_mut().enumerate() {
            widgets::card(ui, |ui| {
                egui::Grid::new(("grp", gi)).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label("Учитель");
                    widgets::combo_req(ui, &format!("g_t{gi}"), &mut g.teacher, teachers, 200.0);
                    ui.end_row();
                    ui.label("Предмет");
                    widgets::combo_req(ui, &format!("g_s{gi}"), &mut g.subject, subjects, 200.0);
                    ui.end_row();
                    if n > 1 {
                        ui.label("Название группы");
                        ui.add(egui::TextEdit::singleline(&mut g.label).desired_width(120.0));
                        ui.end_row();
                    }
                });
                ui.label(theme::small("Кабинеты (пусто — любой подходящий, сначала свой кабинет учителя):"));
                widgets::multi_select(ui, &format!("g_r{gi}"), &mut g.rooms, rooms, 120.0);
                if n > 1 && ui.small_button("Убрать группу").clicked() {
                    remove = Some(gi);
                }
            });
        }
        if let Some(gi) = remove {
            l.groups.remove(gi);
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button(theme::bold(if is_new { "Добавить" } else { "Сохранить" })).clicked() {
                save = true;
            }
            if ui.button("Отмена").clicked() {
                cancel = true;
            }
            if !is_new {
                if ui.button("Копия").clicked() {
                    dup = true;
                }
                if ui.button(RichText::new("Удалить").color(theme::bad())).clicked() {
                    delete = true;
                }
            }
        });
        if !is_new && l.groups.len() > 0 {
            ui.label(theme::small("Изменение групп или классов уберёт уже поставленные уроки этой нагрузки из сетки."));
        }
    });

    if save {
        if l.classes.is_empty() || l.groups.iter().any(|g| g.teacher == 0 || g.subject == 0) {
            app.notify("Выберите класс, учителя и предмет", true);
            app.loads_ui.draft = Some((l, is_new));
            return;
        }
        let lid = l.id;
        app.edit(|p| {
            let old = p.school.loads.iter().find(|x| x.id == lid).cloned();
            let structure_changed = old.as_ref().is_some_and(|o| o.classes != l.classes || o.groups.len() != l.groups.len());
            match p.school.loads.iter_mut().find(|x| x.id == lid) {
                Some(x) => *x = l.clone(),
                None => p.school.loads.push(l.clone()),
            }
            for g in &l.groups {
                if let Some(t) = p.school.teachers.iter_mut().find(|t| t.id == g.teacher) {
                    if !t.subjects.contains(&g.subject) {
                        t.subjects.push(g.subject);
                    }
                }
            }
            let mut ids: Vec<Id> = p.timetable.lessons.iter().filter(|x| x.load == lid).map(|x| x.id).collect();
            if !structure_changed {
                // лишние часы снять с конца
                while ids.len() > l.hours_per_week as usize {
                    let last = ids.pop().unwrap();
                    p.timetable.lessons.retain(|x| x.id != last);
                }
                ids.clear();
            }
            p.timetable.lessons.retain(|x| !ids.contains(&x.id));
            p.replacements.retain(|r| !ids.contains(&r.lesson));
        });
        app.loads_ui.selected = Some(lid);
        app.loads_ui.draft = app.project.school.loads.iter().find(|x| x.id == lid).cloned().map(|x| (x, false));
        app.notify("Нагрузка сохранена", false);
    } else if cancel {
        app.loads_ui.selected = None;
    } else if delete {
        let lid = l.id;
        app.edit(|p| {
            p.school.loads.retain(|x| x.id != lid);
            let ids: Vec<Id> = p.timetable.lessons.iter().filter(|x| x.load == lid).map(|x| x.id).collect();
            p.timetable.lessons.retain(|x| x.load != lid);
            p.replacements.retain(|r| !ids.contains(&r.lesson));
        });
        app.loads_ui.selected = None;
        app.notify("Нагрузка удалена (Ctrl+Z — вернуть)", false);
    } else if dup {
        let mut c = l.clone();
        c.id = widgets::next_id(&app.project.school.loads, |x| x.id);
        c.classes.clear();
        app.loads_ui.draft = Some((c, true));
        app.loads_ui.selected = None;
        app.notify("Копия: выберите классы и нажмите «Добавить»", false);
    } else {
        app.loads_ui.draft = Some((l, is_new));
    }
}
