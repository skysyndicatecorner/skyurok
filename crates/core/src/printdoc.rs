// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Модель печатного документа: страницы с заголовком и таблицей.
//! Её рисуют два вывода — Word (.docx) и PDF, поэтому они всегда совпадают.

use crate::model::*;
use crate::substitute::{stats, SubContext};
use crate::validate::{analyze, Severity};
use chrono::NaiveDate;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Classes,
    Teachers,
    School,
    Replacements,
    Tabel,
}

#[derive(Debug, Clone)]
pub struct PrintOptions {
    /// Гриф «Утверждаю» в правом верхнем углу.
    pub approve: bool,
    /// Время звонков в первом столбце.
    pub bells: bool,
    /// Без заливок (чёрно-белая печать).
    pub grayscale: bool,
    pub full_names: bool,
    pub rooms: bool,
    /// Пометка «Черновик» и лист проблем в конце.
    pub draft: bool,
    pub from: NaiveDate,
    pub to: NaiveDate,
    /// Только эти классы / учителя (пусто — все).
    pub only_classes: Vec<Id>,
    pub only_teachers: Vec<Id>,
    /// Печатать причины отсутствия («больничный», «курсы»). По умолчанию нет:
    /// лист замен висит на виду, а причина — личные сведения.
    pub reasons: bool,
}

impl PrintOptions {
    pub fn new(from: NaiveDate, to: NaiveDate) -> Self {
        Self {
            approve: true,
            bells: true,
            grayscale: false,
            full_names: false,
            rooms: true,
            draft: false,
            from,
            to,
            only_classes: vec![],
            only_teachers: vec![],
            reasons: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    None,
    Header,
    Problem,
    Replacement,
    Cancel,
}

#[derive(Debug, Clone)]
pub struct DocCell {
    pub lines: Vec<String>,
    pub bold_first: bool,
    pub mark: Mark,
}

impl DocCell {
    pub fn text(s: impl Into<String>) -> Self {
        Self { lines: vec![s.into()], bold_first: false, mark: Mark::None }
    }
    pub fn bold(s: impl Into<String>) -> Self {
        Self { lines: vec![s.into()], bold_first: true, mark: Mark::None }
    }
    pub fn empty() -> Self {
        Self { lines: vec![], bold_first: false, mark: Mark::None }
    }
}

#[derive(Debug, Clone)]
pub struct DocTable {
    /// Относительные ширины столбцов.
    pub widths: Vec<f32>,
    pub header: Vec<String>,
    pub rows: Vec<Vec<DocCell>>,
}

#[derive(Debug, Clone)]
pub struct DocPage {
    pub landscape: bool,
    pub title: String,
    pub subtitle: String,
    pub note: Option<String>,
    /// Ссылка для QR-кода под заголовком (лист замен → страница на сайте).
    pub qr: Option<String>,
    pub table: DocTable,
}

#[derive(Debug, Clone)]
pub struct PrintDoc {
    pub title: String,
    pub school: String,
    pub approve: bool,
    pub grayscale: bool,
    /// Текст плашки «Черновик» (None — чистовик).
    pub draft: Option<String>,
    pub pages: Vec<DocPage>,
}

/// Проблемы расписания, которые стоит показать перед печатью.
pub fn problems(p: &Project) -> Vec<String> {
    let r = analyze(&p.school, &p.timetable);
    let mut v: Vec<String> = r.issues.iter().filter(|i| i.severity == Severity::Error).map(|i| i.message.clone()).collect();
    let ctx = SubContext::new(&p.school, &p.timetable, &p.absences).with_classes(&p.class_absences);
    v.extend(
        crate::substitute::check_replacements(&ctx, &p.replacements)
            .into_iter()
            .filter(|i| i.severity == Severity::Error)
            .map(|i| format!("Замены: {}", i.message)),
    );
    v
}

fn period_label(s: &School, p: u8, building: Option<Id>, bells: bool) -> DocCell {
    let mut c = DocCell::bold(format!("{}", p + 1));
    if bells {
        let b = building.and_then(|b| s.buildings.iter().find(|x| x.id == b)).or(s.buildings.first());
        if let Some(bt) = b.and_then(|b| b.bells.iter().find(|x| x.period == p)) {
            c.lines.push(format!("{}–", bt.start));
            c.lines.push(bt.end.clone());
        }
    }
    c
}

fn teacher_label(ix: &Index, id: Id, full: bool) -> String {
    match ix.teacher(id) {
        Some(t) if full => format!("{} {} {}", t.last_name, t.first_name, t.middle_name).trim().to_string(),
        Some(t) => t.short_name(),
        None => format!("#{id}"),
    }
}

fn short_subj(ix: &Index, id: Id) -> String {
    ix.subject(id).map(|s| if s.short.is_empty() { s.name.clone() } else { s.short.clone() }).unwrap_or_default()
}

pub fn build(p: &Project, sections: &[Section], o: &PrintOptions) -> PrintDoc {
    let s = &p.school;
    let ix = Index::new(s);
    let report = analyze(s, &p.timetable);
    let bad: HashSet<Id> = report.issues.iter().filter(|i| i.severity == Severity::Error).flat_map(|i| i.lessons.clone()).collect();
    let days = s.settings.days_per_week as usize;
    let periods = s.settings.periods_per_day;
    let mut pages = vec![];
    let period_head = if o.bells { "Урок" } else { "№" };
    let first_w = if o.bells { 0.9 } else { 0.4 };

    if sections.contains(&Section::Classes) {
        let mut by: HashMap<(Id, Slot), Vec<&Lesson>> = HashMap::new();
        for l in &p.timetable.lessons {
            if let Some(load) = ix.load(l.load) {
                for c in &load.classes {
                    by.entry((*c, l.slot)).or_default().push(l);
                }
            }
        }
        for c in s.classes.iter().filter(|c| o.only_classes.is_empty() || o.only_classes.contains(&c.id)) {
            let mut rows = vec![];
            for per in 0..periods {
                let mut row = vec![period_label(s, per, c.building, o.bells)];
                for d in 0..days {
                    let ls = by.get(&(c.id, Slot::new(d as u8, per))).cloned().unwrap_or_default();
                    let mut cell = DocCell::empty();
                    for l in &ls {
                        let Some(load) = ix.load(l.load) else { continue };
                        for (gi, g) in load.groups.iter().enumerate() {
                            let mut subj = ix.subject_name(g.subject);
                            if !g.label.is_empty() {
                                subj = format!("{subj} ({})", g.label);
                            }
                            if bad.contains(&l.id) {
                                subj = format!("! {subj}");
                                cell.mark = Mark::Problem;
                            }
                            cell.lines.push(subj);
                            let mut who = teacher_label(&ix, g.teacher, o.full_names);
                            if o.rooms {
                                if let Some(r) = l.rooms.get(gi).and_then(|r| ix.room(*r)) {
                                    who = format!("{who}, каб. {}", r.name);
                                }
                            }
                            cell.lines.push(who);
                        }
                    }
                    cell.bold_first = !cell.lines.is_empty();
                    row.push(cell);
                }
                rows.push(row);
            }
            let mut widths = vec![first_w];
            widths.extend(std::iter::repeat_n(2.0, days));
            let mut header = vec![period_head.to_string()];
            header.extend(DAY_NAMES_FULL[..days].iter().map(|d| d.to_string()));
            let hours = p.timetable.lessons.iter().filter(|l| ix.load(l.load).is_some_and(|ld| ld.classes.contains(&c.id))).count();
            pages.push(DocPage {
                landscape: false,
                title: format!("Расписание уроков {} класса", c.name),
                subtitle: format!("{} уч. · {} {} в неделю", c.students, hours, plural(hours, "урок", "урока", "уроков")),
                note: None,
                qr: None,
                table: DocTable { widths, header, rows },
            });
        }
    }

    if sections.contains(&Section::Teachers) {
        let mut by: HashMap<(Id, Slot), Vec<(&Lesson, usize)>> = HashMap::new();
        for l in &p.timetable.lessons {
            if let Some(load) = ix.load(l.load) {
                for (gi, g) in load.groups.iter().enumerate() {
                    by.entry((g.teacher, l.slot)).or_default().push((l, gi));
                }
            }
        }
        for t in s.teachers.iter().filter(|t| o.only_teachers.is_empty() || o.only_teachers.contains(&t.id)) {
            let mut rows = vec![];
            let mut hours = 0;
            for per in 0..periods {
                let mut row = vec![period_label(s, per, None, o.bells)];
                for d in 0..days {
                    let mut cell = DocCell::empty();
                    for (l, gi) in by.get(&(t.id, Slot::new(d as u8, per))).cloned().unwrap_or_default() {
                        let Some(load) = ix.load(l.load) else { continue };
                        hours += 1;
                        let mut head = format!("{} {}", ix.classes_label(&load.classes), short_subj(&ix, load.groups[gi].subject));
                        if bad.contains(&l.id) {
                            head = format!("! {head}");
                            cell.mark = Mark::Problem;
                        }
                        cell.lines.push(head);
                        if o.rooms {
                            if let Some(r) = l.rooms.get(gi) {
                                cell.lines.push(format!("каб. {}", ix.room_name(*r)));
                            }
                        }
                    }
                    cell.bold_first = !cell.lines.is_empty();
                    row.push(cell);
                }
                rows.push(row);
            }
            let mut widths = vec![first_w];
            widths.extend(std::iter::repeat_n(2.0, days));
            let mut header = vec![period_head.to_string()];
            header.extend(DAY_NAMES_FULL[..days].iter().map(|d| d.to_string()));
            pages.push(DocPage {
                landscape: false,
                title: teacher_label(&ix, t.id, true),
                subtitle: format!("{} {} в неделю", hours, plural(hours as usize, "урок", "урока", "уроков")),
                note: None,
                qr: None,
                table: DocTable { widths, header, rows },
            });
        }
    }

    if sections.contains(&Section::School) {
        for d in 0..days {
            let mut rows = vec![];
            for c in &s.classes {
                let mut row = vec![DocCell::bold(c.name.clone())];
                for per in 0..periods {
                    let mut cell = DocCell::empty();
                    for l in p.timetable.lessons.iter().filter(|l| l.slot == Slot::new(d as u8, per)) {
                        let Some(load) = ix.load(l.load) else { continue };
                        if !load.classes.contains(&c.id) {
                            continue;
                        }
                        for g in &load.groups {
                            let mut subj = short_subj(&ix, g.subject);
                            if bad.contains(&l.id) {
                                subj = format!("! {subj}");
                                cell.mark = Mark::Problem;
                            }
                            cell.lines.push(subj);
                            cell.lines.push(ix.teacher(g.teacher).map(|t| t.last_name.clone()).unwrap_or_default());
                        }
                    }
                    cell.bold_first = !cell.lines.is_empty();
                    row.push(cell);
                }
                rows.push(row);
            }
            let mut widths = vec![0.8];
            widths.extend(std::iter::repeat_n(1.4, periods as usize));
            let mut header = vec!["Класс".to_string()];
            header.extend((1..=periods).map(|x| format!("{x}")));
            pages.push(DocPage {
                landscape: true,
                title: format!("Общее расписание — {}", DAY_NAMES_FULL[d].to_lowercase()),
                subtitle: String::new(),
                note: None,
                qr: None,
                table: DocTable { widths, header, rows },
            });
        }
    }

    if sections.contains(&Section::Replacements) {
        let ctx = SubContext::new(s, &p.timetable, &p.absences).with_classes(&p.class_absences);
        let mut date = o.from;
        while date <= o.to {
            let mut reps: Vec<&Replacement> = p.replacements.iter().filter(|r| r.date == date).collect();
            reps.sort_by_key(|r| (ctx.rep_lesson(r).map(|l| l.slot.period), r.lesson));
            if !reps.is_empty() {
                let mut rows = vec![];
                for r in reps {
                    let Some(l) = ctx.rep_lesson(r) else { continue };
                    let Some(load) = ix.load(l.load) else { continue };
                    let g = &load.groups[r.group.min(load.groups.len() - 1)];
                    let (now, subj, mark) = match &r.kind {
                        ReplacementKind::Substitute { teacher, subject } => {
                            (teacher_label(&ix, *teacher, o.full_names), ix.subject_name(*subject), Mark::Replacement)
                        }
                        ReplacementKind::Merge { with_lesson, group } => {
                            let t = ctx
                                .lesson(*with_lesson)
                                .and_then(|x| ix.load(x.load))
                                .map(|ld| {
                                    format!(
                                        "объединение: {} ({})",
                                        teacher_label(&ix, ld.groups[(*group).min(ld.groups.len() - 1)].teacher, false),
                                        ix.classes_label(&ld.classes)
                                    )
                                })
                                .unwrap_or_default();
                            (t, String::new(), Mark::Replacement)
                        }
                        ReplacementKind::Cancel => ("урок снят".into(), String::new(), Mark::Cancel),
                    };
                    let room = r.room.or_else(|| l.rooms.get(r.group).copied()).map(|x| ix.room_name(x)).unwrap_or_default();
                    let mut cls = DocCell::bold(ix.classes_label(&load.classes));
                    if !g.label.is_empty() {
                        cls.lines.push(g.label.clone());
                    }
                    let mut now_c = DocCell::text(now);
                    now_c.mark = mark;
                    rows.push(vec![
                        DocCell::bold(format!("{}", l.slot.period + 1)),
                        cls,
                        DocCell {
                            lines: vec![teacher_label(&ix, r.original_teacher, o.full_names), ix.subject_name(g.subject)],
                            bold_first: false,
                            mark: Mark::None,
                        },
                        now_c,
                        DocCell::text(subj),
                        DocCell::text(room),
                        DocCell::text(if r.confirmed { r.note.clone() } else { format!("{} (черновик)", r.note).trim().to_string() }),
                    ]);
                }
                let absent: Vec<String> =
                    p.absences
                        .iter()
                        .filter(|a| date >= a.from && date <= a.to)
                        .map(|a| {
                            let mut x = teacher_label(&ix, a.teacher, o.full_names);
                            if let Some(ps) = &a.periods {
                                x.push_str(&format!(" ({} ур.)", ps.iter().map(|p| (p + 1).to_string()).collect::<Vec<_>>().join(",")));
                            }
                            if o.reasons && !a.reason.is_empty() {
                                x.push_str(&format!(" — {}", a.reason));
                            }
                            x
                        })
                        .chain(p.class_absences.iter().filter(|a| date >= a.from && date <= a.to).map(|a| {
                            format!("{} класс — {}", ix.class_name(a.class), if a.reason.is_empty() { "отсутствует".into() } else { a.reason.clone() })
                        }))
                        .collect();
                pages.push(DocPage {
                    landscape: false,
                    title: format!("Замены на {}", date.format("%d.%m.%Y")),
                    subtitle: day_of(date, 7).map(|d| DAY_NAMES_FULL[d as usize].to_string()).unwrap_or_default(),
                    note: if absent.is_empty() { None } else { Some(format!("Отсутствуют: {}", absent.join("; "))) },
                    qr: Some(s.settings.public_url.trim().to_string()).filter(|u| !u.is_empty()),
                    table: DocTable {
                        widths: vec![0.5, 0.8, 1.8, 1.8, 1.3, 1.0, 1.6],
                        header: ["Урок", "Класс", "Было", "Стало", "Предмет", "Кабинет", "Примечание"].iter().map(|x| x.to_string()).collect(),
                        rows,
                    },
                });
            }
            date = date.succ_opt().unwrap();
        }
    }

    if sections.contains(&Section::Tabel) {
        let ctx = SubContext::new(s, &p.timetable, &p.absences).with_classes(&p.class_absences);
        let st = stats(&ctx, &p.replacements, o.from, o.to);
        let rows = st
            .iter()
            .map(|x| {
                vec![
                    DocCell::text(teacher_label(&ix, x.teacher, true)),
                    DocCell::text(x.substituted.to_string()),
                    DocCell::text(x.replaced.to_string()),
                    DocCell::empty(),
                ]
            })
            .collect();
        pages.push(DocPage {
            landscape: false,
            title: "Табель замен".into(),
            subtitle: format!("{} — {}", o.from.format("%d.%m.%Y"), o.to.format("%d.%m.%Y")),
            note: None,
            qr: None,
            table: DocTable {
                widths: vec![3.0, 1.2, 1.2, 1.4],
                header: ["Учитель", "Провёл замен, ч", "Его уроков заменено", "Подпись"].iter().map(|x| x.to_string()).collect(),
                rows,
            },
        });
    }

    let probs = problems(p);
    let draft = if o.draft && !probs.is_empty() {
        let unpl = report.stats.unplaced;
        Some(format!(
            "ЧЕРНОВИК: в расписании {} {}{}. Полный список — на последней странице.",
            probs.len(),
            plural(probs.len(), "проблема", "проблемы", "проблем"),
            if unpl > 0 { format!(", не расставлено {unpl} ч") } else { String::new() }
        ))
    } else {
        None
    };
    if draft.is_some() {
        let rows = probs
            .iter()
            .enumerate()
            .map(|(i, m)| vec![DocCell::text(format!("{}", i + 1)), DocCell { lines: vec![m.clone()], bold_first: false, mark: Mark::Problem }])
            .collect();
        pages.push(DocPage {
            landscape: false,
            title: "Проблемы в расписании".into(),
            subtitle: "Документ выгружен как черновик: эти проблемы ещё не исправлены".into(),
            note: None,
            qr: None,
            table: DocTable { widths: vec![0.4, 6.0], header: vec!["№".into(), "Описание".into()], rows },
        });
    }
    PrintDoc { title: "Расписание".into(), school: s.name.clone(), approve: o.approve, grayscale: o.grayscale, draft, pages }
}

pub fn plural(n: usize, one: &str, few: &str, many: &str) -> String {
    let (m10, m100) = (n % 10, n % 100);
    if m10 == 1 && m100 != 11 {
        one.into()
    } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
        few.into()
    } else {
        many.into()
    }
}
