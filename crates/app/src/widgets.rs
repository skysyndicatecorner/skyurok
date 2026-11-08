// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::theme;
use chrono::{Datelike, NaiveDate};
use eframe::egui::{self, Color32, RichText, Vec2};
use skyurok_core::model::*;
use std::collections::HashSet;

/// Недельный график: клик по ячейке меняет состояние, по заголовку — весь день/урок.
/// `tri` — с состоянием «нежелательно» (для учителей).
pub fn avail_grid(ui: &mut egui::Ui, id: &str, a: &mut Availability, days: u8, periods: u8, tri: bool) -> bool {
    let mut changed = false;
    let color = |v: Avail| match v {
        Avail::Yes => theme::ok_bg(),
        Avail::IfNeeded => theme::warn_bg(),
        Avail::No => theme::bad_bg(),
    };
    let next = |v: Avail| match (v, tri) {
        (Avail::Yes, true) => Avail::IfNeeded,
        (Avail::Yes, false) => Avail::No,
        (Avail::IfNeeded, _) => Avail::No,
        (Avail::No, _) => Avail::Yes,
    };
    let set = |a: &mut Availability, s: Slot, v: Avail| {
        a.blocked.retain(|x| *x != s);
        a.if_needed.retain(|x| *x != s);
        a.days_off.retain(|d| *d != s.day || v == Avail::No);
        match v {
            Avail::Yes => {}
            Avail::IfNeeded => a.if_needed.push(s),
            Avail::No => a.blocked.push(s),
        }
    };
    // нормализуем days_off в blocked, чтобы клики работали по ячейкам
    let off: Vec<u8> = std::mem::take(&mut a.days_off);
    for d in off {
        for p in 0..periods {
            let s = Slot::new(d, p);
            if !a.blocked.contains(&s) {
                a.blocked.push(s);
            }
        }
    }
    egui::Grid::new(id).spacing([3.0, 3.0]).show(ui, |ui| {
        ui.label("");
        for d in 0..days {
            let b = ui.add(egui::Button::new(DAY_NAMES[d as usize]).small().min_size(Vec2::new(34.0, 20.0)));
            if b.clicked() {
                let v = next(a.get(Slot::new(d, 0)));
                for p in 0..periods {
                    set(a, Slot::new(d, p), v);
                }
                changed = true;
            }
        }
        ui.end_row();
        for p in 0..periods {
            if ui.add(egui::Button::new(format!("{}", p + 1)).small().min_size(Vec2::new(24.0, 20.0))).clicked() {
                let v = next(a.get(Slot::new(0, p)));
                for d in 0..days {
                    set(a, Slot::new(d, p), v);
                }
                changed = true;
            }
            for d in 0..days {
                let s = Slot::new(d, p);
                let v = a.get(s);
                let txt = match v {
                    Avail::Yes => "",
                    Avail::IfNeeded => "?",
                    Avail::No => "✕",
                };
                if ui.add(egui::Button::new(txt).fill(color(v)).min_size(Vec2::new(34.0, 20.0))).clicked() {
                    set(a, s, next(v));
                    changed = true;
                }
            }
            ui.end_row();
        }
    });
    ui.label(theme::small(if tri {
        "Зелёный — работает, жёлтый «?» — нежелательно, красный — не работает. Клик по дню или номеру — вся строка/столбец."
    } else {
        "Зелёный — можно, красный — нельзя. Клик по дню или номеру — вся строка/столбец."
    }));
    changed
}

pub fn combo_opt(ui: &mut egui::Ui, id: &str, cur: &mut Option<Id>, items: &[(Id, String)], none: &str) -> bool {
    let text = cur.and_then(|c| items.iter().find(|x| x.0 == c)).map(|x| x.1.clone()).unwrap_or_else(|| none.to_string());
    let mut changed = false;
    egui::ComboBox::from_id_salt(id).selected_text(text).width(220.0).show_ui(ui, |ui| {
        if ui.selectable_label(cur.is_none(), none).clicked() {
            *cur = None;
            changed = true;
        }
        for (i, n) in items {
            if ui.selectable_label(*cur == Some(*i), n).clicked() {
                *cur = Some(*i);
                changed = true;
            }
        }
    });
    changed
}

pub fn combo_req(ui: &mut egui::Ui, id: &str, cur: &mut Id, items: &[(Id, String)], width: f32) -> bool {
    let text = items.iter().find(|x| x.0 == *cur).map(|x| x.1.clone()).unwrap_or_else(|| "— выберите —".into());
    let mut changed = false;
    egui::ComboBox::from_id_salt(id).selected_text(text).width(width).show_ui(ui, |ui| {
        for (i, n) in items {
            if ui.selectable_label(*cur == *i, n).clicked() {
                *cur = *i;
                changed = true;
            }
        }
    });
    changed
}

/// Список с галочками (несколько значений).
pub fn multi_select(ui: &mut egui::Ui, id: &str, sel: &mut Vec<Id>, items: &[(Id, String)], height: f32) -> bool {
    let mut changed = false;
    egui::ScrollArea::vertical().id_salt(id).max_height(height).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for (i, n) in items {
                let mut on = sel.contains(i);
                if ui.checkbox(&mut on, n).changed() {
                    if on {
                        sel.push(*i);
                    } else {
                        sel.retain(|x| x != i);
                    }
                    changed = true;
                }
            }
        });
    });
    changed
}

pub fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%d.%m.%Y").ok().or_else(|| NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok())
}

pub fn fmt_date(d: NaiveDate) -> String {
    d.format("%d.%m.%Y").to_string()
}

/// Поле даты с кнопками ◀ ▶. `buf` — текст поля (хранится снаружи между кадрами).
pub fn date_field(ui: &mut egui::Ui, date: &mut NaiveDate, buf: &mut String) -> bool {
    let mut changed = false;
    if ui.small_button("◀").clicked() {
        *date = date.pred_opt().unwrap();
        changed = true;
    }
    if buf.is_empty() || (!changed && parse_date(buf) != Some(*date) && !ui.memory(|m| m.focused().is_some())) {
        *buf = fmt_date(*date);
    }
    let r = ui.add(egui::TextEdit::singleline(buf).desired_width(92.0));
    if r.lost_focus() || r.changed() {
        if let Some(d) = parse_date(buf) {
            if d != *date {
                *date = d;
                changed = true;
            }
        }
    }
    if ui.small_button("▶").clicked() {
        *date = date.succ_opt().unwrap();
        changed = true;
    }
    if changed {
        *buf = fmt_date(*date);
    }
    changed
}

const MONTHS: [&str; 12] = ["Январь", "Февраль", "Март", "Апрель", "Май", "Июнь", "Июль", "Август", "Сентябрь", "Октябрь", "Ноябрь", "Декабрь"];

/// Календарь месяца. Отмеченные даты — жирным с цветом.
pub fn mini_calendar(ui: &mut egui::Ui, date: &mut NaiveDate, marked: &HashSet<NaiveDate>, today: NaiveDate) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        if ui.small_button("‹").clicked() {
            *date = shift_month(*date, -1);
            changed = true;
        }
        ui.label(RichText::new(format!("{} {}", MONTHS[date.month0() as usize], date.year())).family(crate::theme::bold_family()));
        if ui.small_button("›").clicked() {
            *date = shift_month(*date, 1);
            changed = true;
        }
    });
    let first = NaiveDate::from_ymd_opt(date.year(), date.month(), 1).unwrap();
    let offset = first.weekday().num_days_from_monday() as i64;
    egui::Grid::new("mini_cal").spacing([2.0, 2.0]).show(ui, |ui| {
        for d in DAY_NAMES {
            ui.label(theme::small(d));
        }
        ui.end_row();
        for cell in 0..42i64 {
            let day = first + chrono::Duration::days(cell - offset);
            if day.month() != date.month() {
                ui.label("");
            } else {
                let mut txt = RichText::new(day.day().to_string());
                if marked.contains(&day) {
                    txt = txt.family(crate::theme::bold_family()).color(theme::bad());
                }
                if day == today {
                    txt = txt.underline();
                }
                let b = egui::Button::new(txt).min_size(Vec2::new(28.0, 22.0)).fill(if day == *date { theme::sel_bg() } else { Color32::TRANSPARENT });
                if ui.add(b).clicked() {
                    *date = day;
                    changed = true;
                }
            }
            if cell % 7 == 6 {
                ui.end_row();
            }
        }
    });
    changed
}

fn shift_month(d: NaiveDate, delta: i32) -> NaiveDate {
    let m = d.month0() as i32 + delta;
    let y = d.year() + m.div_euclid(12);
    let m = m.rem_euclid(12) as u32 + 1;
    let day = d.day().min(28);
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

pub fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(text).heading());
    ui.add_space(2.0);
}

pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(theme::surface())
        .stroke(egui::Stroke::new(1.0, theme::border()))
        .corner_radius(6.0)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui)
        })
        .inner
}

pub fn next_id<T>(items: &[T], f: impl Fn(&T) -> Id) -> Id {
    items.iter().map(f).max().unwrap_or(0) + 1
}

/// Строка списка: заголовок и подпись, выравнивание влево.
pub fn list_row(ui: &mut egui::Ui, selected: bool, title: &str, subtitle: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 42.0), egui::Sense::click());
    let bg = if selected {
        theme::sel_bg()
    } else if resp.hovered() {
        theme::hover()
    } else {
        Color32::TRANSPARENT
    };
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 5.0, bg);
    p.text(rect.left_top() + Vec2::new(10.0, 5.0), egui::Align2::LEFT_TOP, title, egui::FontId::new(14.5, theme::bold_family()), theme::text());
    p.text(rect.left_top() + Vec2::new(10.0, 24.0), egui::Align2::LEFT_TOP, subtitle, egui::FontId::proportional(12.0), theme::muted());
    resp
}
