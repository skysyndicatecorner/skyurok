// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Печать и экспорт: HTML-страницы, готовые к печати (A4, разрывы страниц),
//! и CSV (открывается в Excel/LibreOffice). Расписание и замены — отдельно.

use crate::model::*;
use crate::substitute::{stats, SubContext};
use chrono::NaiveDate;
use std::collections::HashMap;
use std::fmt::Write;

const CSS: &str = r#"
*{box-sizing:border-box}body{font-family:"PT Sans","Segoe UI",Arial,sans-serif;margin:24px;color:#1b1b1b}
h1{font-size:20px;margin:0 0 4px}h2{font-size:16px;margin:18px 0 6px}.sub{color:#666;font-size:12px;margin-bottom:12px}
table{border-collapse:collapse;width:100%;font-size:12px;margin-bottom:8px}
th,td{border:1px solid #9a9a9a;padding:3px 5px;vertical-align:top}th{background:#eef1f5;font-weight:600}
td.n{width:28px;text-align:center;color:#555}td.e{background:#fafafa}
.t{color:#444;font-size:11px}.r{color:#666;font-size:11px}
.rep{background:#fff4d6}.cancel{background:#fde2e2;text-decoration:line-through}
.page{page-break-after:always;break-after:page}.page:last-child{page-break-after:auto}
.small{font-size:11px;color:#666}
@media print{body{margin:10mm}@page{size:A4;margin:8mm}.noprint{display:none}}
@media print and (orientation:landscape){@page{size:A4 landscape}}
"#;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn doc(title: &str, body: &str) -> String {
    format!(
        "<!DOCTYPE html><html lang=\"ru\"><head><meta charset=\"utf-8\"><title>{}</title><style>{}</style></head><body>{}</body></html>",
        esc(title),
        CSS,
        body
    )
}

fn bells_label(school: &School, period: u8) -> String {
    school
        .buildings
        .first()
        .and_then(|b| b.bells.iter().find(|x| x.period == period))
        .map(|b| format!("<div class=small>{}–{}</div>", b.start, b.end))
        .unwrap_or_default()
}

fn lesson_text(ix: &Index, l: &Lesson, show_teacher: bool, show_classes: bool) -> String {
    let Some(load) = ix.load(l.load) else {
        return String::new();
    };
    let mut parts = vec![];
    for (gi, g) in load.groups.iter().enumerate() {
        let subj = ix.subject(g.subject).map(|s| if s.short.is_empty() { s.name.clone() } else { s.short.clone() }).unwrap_or_default();
        let mut s = esc(&subj);
        if !g.label.is_empty() {
            let _ = write!(s, " <span class=small>({})</span>", esc(&g.label));
        }
        if show_classes {
            let _ = write!(s, " <b>{}</b>", esc(&ix.classes_label(&load.classes)));
        }
        if show_teacher {
            let _ = write!(s, "<div class=t>{}</div>", esc(&ix.teacher_name(g.teacher)));
        }
        if let Some(r) = l.rooms.get(gi) {
            let _ = write!(s, "<div class=r>каб. {}</div>", esc(&ix.room_name(*r)));
        }
        parts.push(s);
    }
    parts.join("<hr style='border:0;border-top:1px dashed #bbb;margin:2px 0'>")
}

fn week_table<F: Fn(Slot) -> String>(school: &School, cell: F) -> String {
    let s = &school.settings;
    let mut h = String::from("<table><tr><th>№</th>");
    for d in 0..s.days_per_week {
        let _ = write!(h, "<th>{}</th>", DAY_NAMES_FULL[d as usize]);
    }
    h.push_str("</tr>");
    for p in 0..s.periods_per_day {
        let _ = write!(h, "<tr><td class=n>{}{}</td>", p + 1, bells_label(school, p));
        for d in 0..s.days_per_week {
            let c = cell(Slot::new(d, p));
            if c.is_empty() {
                h.push_str("<td class=e></td>");
            } else {
                let _ = write!(h, "<td>{c}</td>");
            }
        }
        h.push_str("</tr>");
    }
    h.push_str("</table>");
    h
}

/// Расписание по классам (каждый класс — отдельная страница).
pub fn html_classes(p: &Project, only: Option<&[Id]>) -> String {
    let ix = Index::new(&p.school);
    let mut by: HashMap<(Id, Slot), Vec<&Lesson>> = HashMap::new();
    for l in &p.timetable.lessons {
        if let Some(load) = ix.load(l.load) {
            for c in &load.classes {
                by.entry((*c, l.slot)).or_default().push(l);
            }
        }
    }
    let mut body = String::new();
    for c in &p.school.classes {
        if only.is_some_and(|o| !o.contains(&c.id)) {
            continue;
        }
        let _ = write!(body, "<div class=page><h1>Расписание {} класса</h1><div class=sub>{}</div>", esc(&c.name), esc(&p.school.name));
        body.push_str(&week_table(&p.school, |s| {
            by.get(&(c.id, s)).map(|v| v.iter().map(|l| lesson_text(&ix, l, true, false)).collect::<Vec<_>>().join("<br>")).unwrap_or_default()
        }));
        body.push_str("</div>");
    }
    doc("Расписание классов", &body)
}

/// Расписание учителей.
pub fn html_teachers(p: &Project, only: Option<&[Id]>) -> String {
    let ix = Index::new(&p.school);
    let mut by: HashMap<(Id, Slot), Vec<&Lesson>> = HashMap::new();
    for l in &p.timetable.lessons {
        if let Some(load) = ix.load(l.load) {
            for g in &load.groups {
                by.entry((g.teacher, l.slot)).or_default().push(l);
            }
        }
    }
    let mut body = String::new();
    for t in &p.school.teachers {
        if only.is_some_and(|o| !o.contains(&t.id)) {
            continue;
        }
        let hours = p.timetable.lessons.iter().filter(|l| ix.load(l.load).is_some_and(|ld| ld.groups.iter().any(|g| g.teacher == t.id))).count();
        let _ = write!(body, "<div class=page><h1>{}</h1><div class=sub>{} · уроков в неделю: {}</div>", esc(&t.short_name()), esc(&p.school.name), hours);
        body.push_str(&week_table(&p.school, |s| {
            by.get(&(t.id, s))
                .map(|v| {
                    v.iter()
                        .map(|l| {
                            let load = ix.load(l.load).unwrap();
                            let gi = load.groups.iter().position(|g| g.teacher == t.id).unwrap_or(0);
                            let g = &load.groups[gi];
                            let room = l.rooms.get(gi).map(|r| ix.room_name(*r)).unwrap_or_default();
                            format!(
                                "<b>{}</b> {}<div class=r>каб. {}</div>",
                                esc(&ix.classes_label(&load.classes)),
                                esc(&ix.subject(g.subject).map(|s| s.short.clone()).unwrap_or_default()),
                                esc(&room)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("<br>")
                })
                .unwrap_or_default()
        }));
        body.push_str("</div>");
    }
    doc("Расписание учителей", &body)
}

/// Общее расписание школы: строки — классы, столбцы — дни×уроки.
pub fn html_school(p: &Project) -> String {
    let ix = Index::new(&p.school);
    let s = &p.school.settings;
    let mut by: HashMap<(Id, Slot), Vec<&Lesson>> = HashMap::new();
    for l in &p.timetable.lessons {
        if let Some(load) = ix.load(l.load) {
            for c in &load.classes {
                by.entry((*c, l.slot)).or_default().push(l);
            }
        }
    }
    let mut body = format!("<h1>Общее расписание</h1><div class=sub>{}</div>", esc(&p.school.name));
    for d in 0..s.days_per_week {
        let _ = write!(body, "<div class=page><h2>{}</h2><table><tr><th>Класс</th>", DAY_NAMES_FULL[d as usize]);
        for pp in 0..s.periods_per_day {
            let _ = write!(body, "<th>{}</th>", pp + 1);
        }
        body.push_str("</tr>");
        for c in &p.school.classes {
            let _ = write!(body, "<tr><th>{}</th>", esc(&c.name));
            for pp in 0..s.periods_per_day {
                let cell = by
                    .get(&(c.id, Slot::new(d, pp)))
                    .map(|v| v.iter().map(|l| lesson_text(&ix, l, true, false)).collect::<Vec<_>>().join("<br>"))
                    .unwrap_or_default();
                let _ = write!(body, "<td{}>{}</td>", if cell.is_empty() { " class=e" } else { "" }, cell);
            }
            body.push_str("</tr>");
        }
        body.push_str("</table></div>");
    }
    doc("Общее расписание", &body)
}

fn rep_kind_text(ix: &Index, ctx: &SubContext, r: &Replacement) -> (String, String) {
    match &r.kind {
        ReplacementKind::Substitute { teacher, subject } => (ix.teacher_name(*teacher), ix.subject_name(*subject)),
        ReplacementKind::Merge { with_lesson, group } => {
            let txt = ctx
                .lesson(*with_lesson)
                .and_then(|l| ix.load(l.load))
                .map(|ld| {
                    let g = &ld.groups[*group.min(&(ld.groups.len() - 1))];
                    format!("объединение: {} ({})", ix.teacher_name(g.teacher), ix.classes_label(&ld.classes))
                })
                .unwrap_or_else(|| "объединение".into());
            (txt, String::new())
        }
        ReplacementKind::Cancel => ("урок снят".into(), String::new()),
    }
}

/// Лист замен на интервал дат — отдельный документ, по дням.
pub fn html_replacements(p: &Project, from: NaiveDate, to: NaiveDate) -> String {
    let ix = Index::new(&p.school);
    let ctx = SubContext::new(&p.school, &p.timetable, &p.absences).with_classes(&p.class_absences);
    let mut reps: Vec<&Replacement> = p.replacements.iter().filter(|r| r.date >= from && r.date <= to).collect();
    reps.sort_by_key(|r| {
        (r.date, ctx.lesson(r.lesson).map(|l| l.slot.period), ctx.lesson(r.lesson).and_then(|l| ix.load(l.load)).map(|ld| ix.classes_label(&ld.classes)))
    });
    let mut body = String::new();
    let mut date = from;
    while date <= to {
        let day: Vec<&&Replacement> = reps.iter().filter(|r| r.date == date).collect();
        if !day.is_empty() {
            let wd = day_of(date, 7).map(|d| DAY_NAMES_FULL[d as usize]).unwrap_or("");
            let _ = write!(
                body,
                "<div class=page><h1>Замены на {} ({})</h1><div class=sub>{}</div>",
                date.format("%d.%m.%Y"),
                wd.to_lowercase(),
                esc(&p.school.name)
            );
            let absent: Vec<String> = p
                .absences
                .iter()
                .filter(|a| date >= a.from && date <= a.to)
                .map(|a| {
                    let mut s = ix.teacher_name(a.teacher);
                    if !a.reason.is_empty() {
                        s.push_str(&format!(" — {}", a.reason));
                    }
                    s
                })
                .collect();
            if !absent.is_empty() {
                let _ = write!(body, "<p><b>Отсутствуют:</b> {}</p>", esc(&absent.join("; ")));
            }
            body.push_str("<table><tr><th>Урок</th><th>Класс</th><th>Было</th><th>Стало</th><th>Предмет</th><th>Кабинет</th><th>Примечание</th></tr>");
            for r in day {
                let Some(l) = ctx.lesson(r.lesson) else {
                    continue;
                };
                let Some(load) = ix.load(l.load) else {
                    continue;
                };
                let g = &load.groups[r.group.min(load.groups.len() - 1)];
                let (who, subj) = rep_kind_text(&ix, &ctx, r);
                let room = r.room.or_else(|| l.rooms.get(r.group).copied()).map(|x| ix.room_name(x)).unwrap_or_default();
                let class = if r.kind == ReplacementKind::Cancel { " class=cancel" } else { " class=rep" };
                let _ = write!(
                    body,
                    "<tr{class}><td class=n>{}</td><td><b>{}</b>{}</td><td>{}<div class=small>{}</div></td><td>{}</td><td>{}</td><td>{}</td><td class=small>{}{}</td></tr>",
                    l.slot.period + 1,
                    esc(&ix.classes_label(&load.classes)),
                    if g.label.is_empty() { String::new() } else { format!(" <span class=small>{}</span>", esc(&g.label)) },
                    esc(&ix.teacher_name(r.original_teacher)),
                    esc(&ix.subject_name(g.subject)),
                    esc(&who),
                    esc(&subj),
                    esc(&room),
                    esc(&r.note),
                    if r.confirmed { "" } else { " · черновик" }
                );
            }
            body.push_str("</table></div>");
        }
        date = date.succ_opt().unwrap();
    }
    // табель
    let st = stats(&ctx, &p.replacements, from, to);
    if !st.is_empty() {
        let _ = write!(
            body,
            "<div class=page><h1>Табель замен</h1><div class=sub>{} — {}</div><table><tr><th>Учитель</th><th>Провёл замен (ч)</th><th>Его уроков заменено/снято</th></tr>",
            from.format("%d.%m.%Y"),
            to.format("%d.%m.%Y")
        );
        for s in st {
            let _ = write!(body, "<tr><td>{}</td><td>{}</td><td>{}</td></tr>", esc(&ix.teacher_name(s.teacher)), s.substituted, s.replaced);
        }
        body.push_str("</table></div>");
    }
    if body.is_empty() {
        body = "<h1>Замен нет</h1>".into();
    }
    doc("Замены", &body)
}

/// Расписание на конкретный день с учётом замен (для стенда/киоска, рассылки).
pub fn html_day(p: &Project, date: NaiveDate) -> String {
    let ix = Index::new(&p.school);
    let ctx = SubContext::new(&p.school, &p.timetable, &p.absences).with_classes(&p.class_absences);
    let s = &p.school.settings;
    let Some(day) = day_of(date, s.days_per_week) else {
        return doc("Выходной", "<h1>Выходной день</h1>");
    };
    let mut body = format!("<h1>Расписание на {}</h1><div class=sub>{}</div><table><tr><th>Класс</th>", date.format("%d.%m.%Y"), esc(&p.school.name));
    for pp in 0..s.periods_per_day {
        let _ = write!(body, "<th>{}</th>", pp + 1);
    }
    body.push_str("</tr>");
    for c in &p.school.classes {
        let _ = write!(body, "<tr><th>{}</th>", esc(&c.name));
        for pp in 0..s.periods_per_day {
            let mut cell = String::new();
            let mut cls = "";
            for l in p.timetable.lessons.iter().filter(|l| l.slot == Slot::new(day, pp)) {
                let Some(load) = ix.load(l.load) else {
                    continue;
                };
                if !load.classes.contains(&c.id) {
                    continue;
                }
                for (gi, g) in load.groups.iter().enumerate() {
                    let rep = p.replacements.iter().find(|r| r.date == date && r.lesson == l.id && r.group == gi);
                    let subj = ix.subject(g.subject).map(|s| s.short.clone()).unwrap_or_default();
                    match rep {
                        None => {
                            let _ = write!(cell, "{}<div class=t>{}</div>", esc(&subj), esc(&ix.teacher_name(g.teacher)));
                        }
                        Some(r) => {
                            let (who, sname) = rep_kind_text(&ix, &ctx, r);
                            if r.kind == ReplacementKind::Cancel {
                                cls = " class=cancel";
                                let _ = write!(cell, "{}<div class=t>снят</div>", esc(&subj));
                            } else {
                                cls = " class=rep";
                                let shown = match &r.kind {
                                    ReplacementKind::Substitute { subject, .. } => ix.subject(*subject).map(|s| s.short.clone()).unwrap_or(sname),
                                    _ => subj.clone(),
                                };
                                let _ = write!(cell, "{}<div class=t>{} (замена)</div>", esc(&shown), esc(&who));
                            }
                        }
                    }
                    let room = rep.and_then(|r| r.room).or_else(|| l.rooms.get(gi).copied());
                    if let Some(r) = room {
                        let _ = write!(cell, "<div class=r>каб. {}</div>", esc(&ix.room_name(r)));
                    }
                }
            }
            let _ = write!(body, "<td{cls}>{cell}</td>");
        }
        body.push_str("</tr>");
    }
    body.push_str("</table><p class=small>Жёлтым — замены, красным — снятые уроки.</p>");
    doc("Расписание на день", &body)
}

fn csv_cell(s: &str) -> String {
    if s.contains([';', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// CSV расписания (разделитель «;», UTF-8 с BOM — Excel открывает корректно).
pub fn csv_timetable(p: &Project) -> String {
    let ix = Index::new(&p.school);
    let mut out = String::from("\u{feff}День;Урок;Класс;Предмет;Группа;Учитель;Кабинет\n");
    for l in &p.timetable.lessons {
        let Some(load) = ix.load(l.load) else {
            continue;
        };
        for (gi, g) in load.groups.iter().enumerate() {
            let row = [
                DAY_NAMES_FULL[l.slot.day as usize].to_string(),
                (l.slot.period + 1).to_string(),
                ix.classes_label(&load.classes),
                ix.subject_name(g.subject),
                g.label.clone(),
                ix.teacher_name(g.teacher),
                l.rooms.get(gi).map(|r| ix.room_name(*r)).unwrap_or_default(),
            ];
            out.push_str(&row.iter().map(|x| csv_cell(x)).collect::<Vec<_>>().join(";"));
            out.push('\n');
        }
    }
    out
}

pub fn csv_replacements(p: &Project, from: NaiveDate, to: NaiveDate) -> String {
    let ix = Index::new(&p.school);
    let ctx = SubContext::new(&p.school, &p.timetable, &p.absences).with_classes(&p.class_absences);
    let mut out = String::from("\u{feff}Дата;Урок;Класс;Было (учитель);Было (предмет);Стало;Предмет;Кабинет;Статус;Примечание\n");
    let mut reps: Vec<&Replacement> = p.replacements.iter().filter(|r| r.date >= from && r.date <= to).collect();
    reps.sort_by_key(|r| (r.date, ctx.lesson(r.lesson).map(|l| l.slot.period)));
    for r in reps {
        let Some(l) = ctx.lesson(r.lesson) else {
            continue;
        };
        let Some(load) = ix.load(l.load) else {
            continue;
        };
        let g = &load.groups[r.group.min(load.groups.len() - 1)];
        let (who, subj) = rep_kind_text(&ix, &ctx, r);
        let row = [
            r.date.format("%d.%m.%Y").to_string(),
            (l.slot.period + 1).to_string(),
            ix.classes_label(&load.classes),
            ix.teacher_name(r.original_teacher),
            ix.subject_name(g.subject),
            who,
            subj,
            r.room.or_else(|| l.rooms.get(r.group).copied()).map(|x| ix.room_name(x)).unwrap_or_default(),
            if r.confirmed { "подтверждена".into() } else { "черновик".into() },
            r.note.clone(),
        ];
        out.push_str(&row.iter().map(|x| csv_cell(x)).collect::<Vec<_>>().join(";"));
        out.push('\n');
    }
    out
}

/// Упрощённый JSON для сайта школы, киоска (SchoolTouch) или бота:
/// расписание по классам и замены за период.
pub fn json_public(p: &Project, from: NaiveDate, to: NaiveDate) -> String {
    use serde_json::json;
    let ix = Index::new(&p.school);
    let ctx = SubContext::new(&p.school, &p.timetable, &p.absences).with_classes(&p.class_absences);
    let bells: Vec<_> = p
        .school
        .buildings
        .iter()
        .map(|b| json!({"building": b.name, "bells": b.bells.iter().map(|x| json!({"period": x.period + 1, "start": x.start, "end": x.end})).collect::<Vec<_>>()}))
        .collect();
    let classes: Vec<_> = p
        .school
        .classes
        .iter()
        .map(|c| {
            let mut lessons = vec![];
            for l in &p.timetable.lessons {
                let Some(load) = ix.load(l.load) else { continue };
                if !load.classes.contains(&c.id) {
                    continue;
                }
                for (gi, g) in load.groups.iter().enumerate() {
                    lessons.push(json!({
                        "day": l.slot.day + 1,
                        "period": l.slot.period + 1,
                        "subject": ix.subject_name(g.subject),
                        "teacher": ix.teacher_name(g.teacher),
                        "room": l.rooms.get(gi).map(|r| ix.room_name(*r)),
                        "group": if g.label.is_empty() { None } else { Some(g.label.clone()) },
                    }));
                }
            }
            json!({"class": c.name, "lessons": lessons})
        })
        .collect();
    let reps: Vec<_> = p
        .replacements
        .iter()
        .filter(|r| r.date >= from && r.date <= to)
        .filter_map(|r| {
            let l = ctx.lesson(r.lesson)?;
            let load = ix.load(l.load)?;
            let g = load.groups.get(r.group)?;
            let (now, subj) = rep_kind_text(&ix, &ctx, r);
            Some(json!({
                "date": r.date.to_string(),
                "period": l.slot.period + 1,
                "class": ix.classes_label(&load.classes),
                "was_teacher": ix.teacher_name(r.original_teacher),
                "was_subject": ix.subject_name(g.subject),
                "now": now,
                "subject": subj,
                "room": r.room.or_else(|| l.rooms.get(r.group).copied()).map(|x| ix.room_name(x)),
                "cancelled": r.kind == ReplacementKind::Cancel,
            }))
        })
        .collect();
    serde_json::to_string_pretty(&json!({
        "school": p.school.name,
        "days": DAY_NAMES_FULL[..p.school.settings.days_per_week as usize],
        "bells": bells,
        "classes": classes,
        "replacements": reps,
    }))
    .unwrap()
}
