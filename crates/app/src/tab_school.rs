// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::app::{default_bells, SkyApp};
use crate::{theme, widgets};
use eframe::egui::{self, RichText};
use skyurok_core::model::*;

#[derive(Default)]
pub struct State {
    draft: Option<Draft>,
}

#[derive(Clone, PartialEq)]
struct Draft {
    name: String,
    settings: Settings,
    buildings: Vec<Building>,
}

pub fn show(app: &mut SkyApp, ui: &mut egui::Ui) {
    let current =
        Draft { name: app.project.school.name.clone(), settings: app.project.school.settings.clone(), buildings: app.project.school.buildings.clone() };
    if app.school_ui.draft.is_none() {
        app.school_ui.draft = Some(current.clone());
    }
    let mut d = app.school_ui.draft.take().unwrap();
    let changed = d != current;
    let mut apply = false;
    let mut reset = false;

    ui.horizontal(|ui| {
        widgets::heading(ui, "Школа");
        ui.add_space(20.0);
        if ui.add_enabled(changed, egui::Button::new(theme::bold("Сохранить изменения"))).clicked() {
            apply = true;
        }
        if ui.add_enabled(changed, egui::Button::new("Отменить")).clicked() {
            reset = true;
        }
        if changed {
            ui.label(RichText::new("есть несохранённые изменения").color(theme::warn()));
        }
    });

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.columns(2, |cols| {
            let ui = &mut cols[0];
            widgets::card(ui, |ui| {
                ui.label(theme::bold("Основное"));
                egui::Grid::new("school_main").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label("Название");
                    ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(320.0));
                    ui.end_row();
                    ui.label("Учебных дней в неделю");
                    ui.add(egui::DragValue::new(&mut d.settings.days_per_week).range(1..=7));
                    ui.end_row();
                    ui.label("Уроков в день (максимум)");
                    ui.add(egui::DragValue::new(&mut d.settings.periods_per_day).range(1..=16));
                    ui.end_row();
                });
                ui.checkbox(&mut d.settings.break_enough_for_building_change, "Перемены хватает, чтобы перейти в другое здание");
                ui.checkbox(&mut d.settings.classes_may_start_later, "Классы могут приходить не к первому уроку (не считать окном)");
            });
            ui.add_space(8.0);
            widgets::card(ui, |ui| {
                ui.label(theme::bold("Гигиенические требования"));
                ui.label(theme::small("Нормы меняются, поэтому профиль настраивается. Сверьте с действующим СанПиН 1.2.3685-21."));
                ui.horizontal(|ui| {
                    ui.label("Название профиля");
                    ui.text_edit_singleline(&mut d.settings.hygiene.name);
                });
                ui.horizontal(|ui| {
                    ui.label("Пик трудности:");
                    for day in 0..d.settings.days_per_week {
                        let mut on = d.settings.hygiene.peak_days.contains(&day);
                        if ui.checkbox(&mut on, DAY_NAMES[day as usize]).changed() {
                            toggle(&mut d.settings.hygiene.peak_days, day, on);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Облегчённый день:");
                    for day in 0..d.settings.days_per_week {
                        let mut on = d.settings.hygiene.light_days.contains(&day);
                        if ui.checkbox(&mut on, DAY_NAMES[day as usize]).changed() {
                            toggle(&mut d.settings.hygiene.light_days, day, on);
                        }
                    }
                });
            });
            ui.add_space(8.0);
            widgets::card(ui, |ui| {
                ui.label(theme::bold("Что важнее при составлении (штрафы)"));
                ui.label(theme::small("Чем больше число, тем сильнее программа избегает нарушения."));
                let w = &mut d.settings.weights;
                egui::Grid::new("weights").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                    let rows: [(&str, &mut i64); 12] = [
                        ("Окно у класса", &mut w.class_window),
                        ("Больше максимума уроков в день", &mut w.class_over_max),
                        ("Меньше минимума уроков в день", &mut w.class_under_min),
                        ("Одиночное окно у учителя", &mut w.teacher_single_window),
                        ("Длинное окно у учителя", &mut w.teacher_multi_window),
                        ("День учителя с одним уроком", &mut w.teacher_one_lesson_day),
                        ("Переход учителя в другое здание", &mut w.teacher_building_change),
                        ("Урок в «нежелательное» время", &mut w.teacher_if_needed_slot),
                        ("Не выдержан методический день", &mut w.teacher_methodical_day_violation),
                        ("Нарушен номер урока (первый/последний)", &mut w.position_violation),
                        ("Пик трудности не в те дни", &mut w.hygiene_peak),
                        ("Самый трудный урок — крайний", &mut w.hygiene_hardest_at_edge),
                    ];
                    for (n, v) in rows {
                        ui.label(n);
                        ui.add(egui::DragValue::new(v).range(0..=500));
                        ui.end_row();
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("Профиль «меньше окон у учителей»").clicked() {
                        d.settings.weights.teacher_single_window = 20;
                        d.settings.weights.teacher_multi_window = 35;
                    }
                    if ui.button("Стандарт").clicked() {
                        d.settings.weights = Weights::default();
                    }
                });
            });

            let ui = &mut cols[1];
            widgets::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::bold("Здания и звонки"));
                    if ui.button("+ Здание").clicked() {
                        let id = widgets::next_id(&d.buildings, |b| b.id);
                        d.buildings.push(Building {
                            id,
                            name: format!("Здание {id}"),
                            address: String::new(),
                            bells: default_bells(d.settings.periods_per_day),
                            travel_minutes: vec![],
                        });
                    }
                });
                let ids: Vec<(Id, String)> = d.buildings.iter().map(|b| (b.id, b.name.clone())).collect();
                let mut remove = None;
                let periods = d.settings.periods_per_day;
                for (bi, b) in d.buildings.iter_mut().enumerate() {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut b.name).desired_width(180.0));
                        ui.add(egui::TextEdit::singleline(&mut b.address).hint_text("адрес").desired_width(200.0));
                        if ids.len() > 1 && ui.small_button("Удалить").clicked() {
                            remove = Some(bi);
                        }
                    });
                    for (oid, oname) in ids.iter().filter(|x| x.0 != b.id) {
                        let mut m = b.travel_minutes.iter().find(|x| x.0 == *oid).map(|x| x.1).unwrap_or(10);
                        ui.horizontal(|ui| {
                            ui.label(format!("Дорога до «{oname}», мин:"));
                            if ui.add(egui::DragValue::new(&mut m).range(0..=120)).changed() {
                                b.travel_minutes.retain(|x| x.0 != *oid);
                                b.travel_minutes.push((*oid, m));
                            }
                        });
                    }
                    egui::CollapsingHeader::new(format!("Звонки ({} ур.)", b.bells.len())).id_salt(("bells", b.id)).show(ui, |ui| {
                        if b.bells.len() < periods as usize {
                            if ui.small_button("Заполнить по умолчанию").clicked() {
                                b.bells = default_bells(periods);
                            }
                        }
                        egui::Grid::new(("bellgrid", b.id)).num_columns(3).show(ui, |ui| {
                            for bell in b.bells.iter_mut().filter(|x| x.period < periods) {
                                ui.label(format!("{} урок", bell.period + 1));
                                ui.add(egui::TextEdit::singleline(&mut bell.start).desired_width(56.0));
                                ui.add(egui::TextEdit::singleline(&mut bell.end).desired_width(56.0));
                                ui.end_row();
                            }
                        });
                    });
                }
                if let Some(i) = remove {
                    d.buildings.remove(i);
                }
                ui.add_space(6.0);
                ui.label(theme::small(
                    "Класс весь день учится в одном здании. Учителю между уроками в разных зданиях нужен свободный урок, если не отмечено «перемены хватает».",
                ));
            });
            ui.add_space(8.0);
            widgets::card(ui, |ui| {
                ui.label(theme::bold("Сводка"));
                let s = &app.project.school;
                ui.label(format!(
                    "Классов: {} · учителей: {} · предметов: {} · кабинетов: {} · нагрузок: {} ({} ч в неделю)",
                    s.classes.len(),
                    s.teachers.len(),
                    s.subjects.len(),
                    s.rooms.len(),
                    s.loads.len(),
                    s.loads.iter().map(|l| l.hours_per_week as u32).sum::<u32>()
                ));
            });
        });
    });

    if apply {
        let nd = d.clone();
        app.edit(|p| {
            p.school.name = nd.name;
            p.school.settings = nd.settings;
            // ссылки на удалённые здания — на первое
            let first = nd.buildings.first().map(|b| b.id).unwrap_or(1);
            let ids: Vec<Id> = nd.buildings.iter().map(|b| b.id).collect();
            for r in &mut p.school.rooms {
                if !ids.contains(&r.building) {
                    r.building = first;
                }
            }
            for c in &mut p.school.classes {
                if c.building.is_some_and(|b| !ids.contains(&b)) {
                    c.building = None;
                }
            }
            p.school.buildings = nd.buildings;
        });
        app.notify("Настройки школы сохранены", false);
        app.school_ui.draft = None;
    } else if reset {
        app.school_ui.draft = None;
    } else {
        app.school_ui.draft = Some(d);
    }
}

fn toggle(v: &mut Vec<u8>, x: u8, on: bool) {
    v.retain(|y| *y != x);
    if on {
        v.push(x);
        v.sort();
    }
}
