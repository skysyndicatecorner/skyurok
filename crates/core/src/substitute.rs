// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Замены: какие уроки затронуты отсутствием, кого поставить вместо
//! (с ранжированием и объяснением), автоподбор на весь период, проверка
//! конфликтов в уже назначенных заменах и статистика для табеля.

use crate::grid::Occupancy;
use crate::model::*;
use crate::validate::{Issue, Severity};
use chrono::{Datelike, NaiveDate};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize)]
pub struct AffectedLesson {
    pub lesson_idx: usize,
    pub lesson_id: Id,
    pub group: usize,
    pub date: NaiveDate,
    pub slot: Slot,
    pub teacher: Id,
}

#[derive(Debug, Clone, Serialize)]
pub struct Suggestion {
    pub kind: ReplacementKind,
    pub room: Option<Id>,
    pub score: i32,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SubPolicy {
    /// Минимальный балл, чтобы автоподбор поставил учителя без вопросов.
    pub min_auto_score: i32,
    /// Разрешать автоподбору снимать крайние уроки.
    pub allow_cancel_edge: bool,
    /// Разрешать звать учителя, у которого в этот день нет уроков.
    pub allow_day_off_teachers: bool,
}

impl SubPolicy {
    pub fn standard() -> Self {
        Self { min_auto_score: 20, allow_cancel_edge: true, allow_day_off_teachers: true }
    }
}

pub struct SubContext<'a> {
    pub school: &'a School,
    pub tt: &'a Timetable,
    pub absences: &'a [Absence],
    pub class_absences: Vec<ClassAbsence>,
    pub ix: Index<'a>,
    occ: Occupancy,
    lesson_by_id: HashMap<Id, usize>,
    /// Какие классы ведёт учитель (по нагрузкам).
    teacher_classes: HashMap<Id, HashSet<Id>>,
}

fn week_key(d: NaiveDate) -> (i32, u32) {
    let w = d.iso_week();
    (w.year(), w.week())
}

impl<'a> SubContext<'a> {
    pub fn new(school: &'a School, tt: &'a Timetable, absences: &'a [Absence]) -> Self {
        let mut teacher_classes: HashMap<Id, HashSet<Id>> = HashMap::new();
        for l in &school.loads {
            for g in &l.groups {
                teacher_classes.entry(g.teacher).or_default().extend(l.classes.iter().copied());
            }
        }
        Self {
            school,
            tt,
            absences,
            class_absences: vec![],
            ix: Index::new(school),
            occ: Occupancy::build(school, tt),
            lesson_by_id: tt.lessons.iter().enumerate().map(|(i, l)| (l.id, i)).collect(),
            teacher_classes,
        }
    }

    /// То же, но с учётом отсутствующих классов.
    pub fn with_classes(mut self, ca: &[ClassAbsence]) -> Self {
        self.class_absences = ca.to_vec();
        self
    }

    /// Все классы урока отсутствуют — урок не нужен, учитель свободен.
    pub fn lesson_classes_absent(&self, l: &Lesson, date: NaiveDate) -> bool {
        let Some(load) = self.ix.load(l.load) else { return false };
        !load.classes.is_empty() && load.classes.iter().all(|c| self.class_absences.iter().any(|a| a.class == *c && a.covers(date, l.slot.period)))
    }

    pub fn lesson(&self, id: Id) -> Option<&'a Lesson> {
        self.lesson_by_id.get(&id).map(|i| &self.tt.lessons[*i])
    }

    pub fn is_absent(&self, t: Id, date: NaiveDate, period: u8) -> bool {
        self.absences.iter().any(|a| a.teacher == t && a.covers(date, period))
    }

    /// Уроки, затронутые отсутствиями в интервале дат.
    pub fn affected(&self, from: NaiveDate, to: NaiveDate) -> Vec<AffectedLesson> {
        let mut out = vec![];
        let mut date = from;
        while date <= to {
            if let Some(day) = day_of(date, self.school.settings.days_per_week) {
                for (li, l) in self.tt.lessons.iter().enumerate() {
                    if l.slot.day != day || self.lesson_classes_absent(l, date) {
                        continue;
                    }
                    let Some(load) = self.ix.load(l.load) else {
                        continue;
                    };
                    for (gi, g) in load.groups.iter().enumerate() {
                        if self.is_absent(g.teacher, date, l.slot.period) {
                            out.push(AffectedLesson { lesson_idx: li, lesson_id: l.id, group: gi, date, slot: l.slot, teacher: g.teacher });
                        }
                    }
                }
            }
            date = date.succ_opt().unwrap();
        }
        out.sort_by_key(|a| (a.date, a.slot.period, a.lesson_id, a.group));
        out
    }

    /// Свой урок учителя в этот день снят/передан (он сам отсутствует или его заменили).
    fn own_lesson_released(&self, t: Id, date: NaiveDate, lr: (Id, usize), reps: &[&Replacement]) -> bool {
        reps.iter().any(|r| r.date == date && r.lesson == lr.0 && r.group == lr.1 && r.original_teacher == t)
    }

    /// Занятость учителя в конкретную дату и урок с учётом замен.
    pub fn busy(&self, t: Id, date: NaiveDate, slot: Slot, reps: &[&Replacement]) -> Option<String> {
        if self.is_absent(t, date, slot.period) {
            return Some("отсутствует".into());
        }
        if let Some(refs) = self.occ.teacher.get(&(t, slot)) {
            for r in refs {
                let l = &self.tt.lessons[r.lesson];
                if !self.own_lesson_released(t, date, (l.id, r.group), reps) && !self.lesson_classes_absent(l, date) {
                    let load = self.ix.load(l.load);
                    let label = load.map(|ld| self.ix.classes_label(&ld.classes)).unwrap_or_default();
                    return Some(format!("свой урок в {label}"));
                }
            }
        }
        for r in reps {
            if r.date != date {
                continue;
            }
            if let ReplacementKind::Substitute { teacher, .. } = r.kind {
                if teacher == t && self.lesson(r.lesson).map(|l| l.slot) == Some(slot) {
                    return Some("уже на другой замене".into());
                }
            }
        }
        None
    }

    /// Номера уроков, которые учитель реально ведёт в эту дату (свои + замены).
    pub fn teacher_periods(&self, t: Id, date: NaiveDate, reps: &[&Replacement]) -> Vec<(u8, Option<Id>)> {
        let Some(day) = day_of(date, self.school.settings.days_per_week) else {
            return vec![];
        };
        let mut out = vec![];
        for p in 0..self.school.settings.periods_per_day {
            let slot = Slot::new(day, p);
            if self.is_absent(t, date, p) {
                continue;
            }
            if let Some(refs) = self.occ.teacher.get(&(t, slot)) {
                for r in refs {
                    let l = &self.tt.lessons[r.lesson];
                    if !self.own_lesson_released(t, date, (l.id, r.group), reps) && !self.lesson_classes_absent(l, date) {
                        let b = l.rooms.get(r.group).and_then(|x| self.ix.room(*x)).map(|x| x.building);
                        out.push((p, b));
                    }
                }
            }
        }
        for r in reps {
            if r.date != date {
                continue;
            }
            if let ReplacementKind::Substitute { teacher, .. } = r.kind {
                if teacher == t {
                    if let Some(l) = self.lesson(r.lesson) {
                        let room = r.room.or_else(|| l.rooms.get(r.group).copied());
                        let b = room.and_then(|x| self.ix.room(x)).map(|x| x.building);
                        out.push((l.slot.period, b));
                    }
                }
            }
        }
        out.sort();
        out.dedup_by_key(|x| x.0);
        out
    }

    /// Периоды класса в эту дату (для оценки «крайний ли урок»).
    fn class_periods(&self, class: Id, date: NaiveDate, reps: &[&Replacement]) -> Vec<u8> {
        let Some(day) = day_of(date, self.school.settings.days_per_week) else {
            return vec![];
        };
        let mut v = vec![];
        for p in 0..self.school.settings.periods_per_day {
            if let Some(ls) = self.occ.class.get(&(class, Slot::new(day, p))) {
                let cancelled = ls.iter().all(|li| {
                    let l = &self.tt.lessons[*li];
                    reps.iter().any(|r| r.date == date && r.lesson == l.id && r.kind == ReplacementKind::Cancel)
                });
                if !cancelled {
                    v.push(p);
                }
            }
        }
        v
    }

    fn subs_count(&self, t: Id, date: NaiveDate, reps: &[&Replacement]) -> (usize, usize) {
        let wk = week_key(date);
        let mut week = 0;
        let mut month = 0;
        for r in reps {
            if let ReplacementKind::Substitute { teacher, .. } = r.kind {
                if teacher == t {
                    if week_key(r.date) == wk {
                        week += 1;
                    }
                    if r.date.month() == date.month() && r.date.year() == date.year() {
                        month += 1;
                    }
                }
            }
        }
        (week, month)
    }

    /// Варианты замены для одного затронутого урока, лучшие — первыми.
    pub fn suggest(&self, a: &AffectedLesson, reps: &[&Replacement], policy: &SubPolicy) -> Vec<Suggestion> {
        let s = &self.school.settings;
        let lesson = &self.tt.lessons[a.lesson_idx];
        let Some(load) = self.ix.load(lesson.load) else {
            return vec![];
        };
        let group = &load.groups[a.group];
        let subject = self.ix.subject(group.subject);
        let area = subject.map(|s| s.area.as_str()).unwrap_or("");
        let room = lesson.rooms.get(a.group).copied();
        let lesson_building = room.and_then(|r| self.ix.room(r)).map(|r| r.building);
        let students = self.ix.students_in(load, a.group);
        let mut out = vec![];

        // 1) Другой учитель.
        for t in &self.school.teachers {
            if t.id == a.teacher || !t.can_substitute {
                continue;
            }
            if self.busy(t.id, a.date, a.slot, reps).is_some() {
                continue;
            }
            let avail = t.availability.get(a.slot);
            let mut score = 0i32;
            let mut reasons = vec![];
            let mut warnings = vec![];
            if avail == Avail::No {
                if !policy.allow_day_off_teachers {
                    continue;
                }
                score -= 60;
                warnings.push("по графику не работает в это время".into());
            } else if avail == Avail::IfNeeded {
                score -= 15;
                warnings.push("нежелательное время по графику".into());
            }
            let freed = self.occ.teacher.get(&(t.id, a.slot)).is_some_and(|v| v.iter().any(|r| self.lesson_classes_absent(&self.tt.lessons[r.lesson], a.date)));
            if freed {
                score += 25;
                reasons.push("свободен: его класс в это время отсутствует".into());
            }
            let (wk, mo) = self.subs_count(t.id, a.date, reps);
            if let Some(max) = t.max_substitutions_per_week {
                if wk >= max as usize {
                    continue;
                }
            }
            // Предмет.
            let mut sub_subject = group.subject;
            if t.subjects.contains(&group.subject) {
                score += 100;
                reasons.push("ведёт этот предмет".into());
            } else if !area.is_empty() && t.subjects.iter().filter_map(|x| self.ix.subject(*x)).any(|x| x.area == area) {
                score += 40;
                reasons.push(format!("смежный предмет (область «{area}»)"));
            } else if let Some(own) = t.subjects.first() {
                sub_subject = *own;
                score += 5;
                reasons.push(format!("проведёт свой предмет: {}", self.ix.subject_name(*own)));
            }
            if load.classes.iter().any(|c| self.teacher_classes.get(&t.id).is_some_and(|s| s.contains(c))) {
                score += 30;
                reasons.push("знает класс".into());
            }
            // День учителя.
            let day = self.teacher_periods(t.id, a.date, reps);
            let p = a.slot.period;
            let before = day.iter().any(|x| x.0 < p);
            let after = day.iter().any(|x| x.0 > p);
            let adjacent: Vec<&(u8, Option<Id>)> = day.iter().filter(|x| x.0 + 1 == p || x.0 == p + 1).collect();
            if before && after {
                score += 50;
                reasons.push("окно в расписании — замена закрывает окно".into());
            } else if !adjacent.is_empty() {
                score += 35;
                reasons.push("урок рядом — задержится/придёт на урок раньше".into());
            } else if !day.is_empty() {
                score += 10;
                reasons.push("работает в этот день".into());
            } else {
                score -= 40;
                warnings.push("в этот день уроков нет — придётся прийти специально".into());
                if !policy.allow_day_off_teachers {
                    continue;
                }
            }
            // Корпуса.
            if let Some(lb) = lesson_building {
                let impossible = adjacent.iter().any(|x| x.1.is_some_and(|b| b != lb)) && !s.break_enough_for_building_change;
                if impossible {
                    continue; // физически не успеть перейти
                }
                if day.iter().any(|x| x.1.is_some_and(|b| b != lb)) {
                    score -= 20;
                    let bn = self.ix.building(lb).map(|b| b.name.clone()).unwrap_or_default();
                    warnings.push(format!("переход в другой корпус ({bn})"));
                } else if !day.is_empty() && self.school.buildings.len() > 1 {
                    score += 10;
                    reasons.push("в том же корпусе".into());
                }
            }
            // Справедливость.
            if wk > 0 || mo > 0 {
                score -= 12 * wk as i32 + 3 * mo as i32;
                reasons.push(format!("замен на этой неделе: {wk}, в месяце: {mo}"));
            }
            out.push(Suggestion { kind: ReplacementKind::Substitute { teacher: t.id, subject: sub_subject }, room, score, reasons, warnings });
        }

        // 2) Объединение групп: учитель другой группы этого же урока берёт обе.
        if load.groups.len() > 1 {
            for (gi, g) in load.groups.iter().enumerate() {
                if gi == a.group || self.is_absent(g.teacher, a.date, a.slot.period) {
                    continue;
                }
                let other_room = lesson.rooms.get(gi).and_then(|r| self.ix.room(*r));
                let total = students + self.ix.students_in(load, gi);
                let mut warnings = vec![];
                let mut score = 70;
                if let Some(r) = other_room {
                    if r.capacity < total {
                        score -= 40;
                        warnings.push(format!("в кабинете {} мест {}, учеников {}", r.name, r.capacity, total));
                    }
                }
                out.push(Suggestion {
                    kind: ReplacementKind::Merge { with_lesson: lesson.id, group: gi },
                    room: other_room.map(|r| r.id),
                    score,
                    reasons: vec![format!("{} ведёт другую группу — объединить группы", self.ix.teacher_name(g.teacher))],
                    warnings,
                });
            }
        }

        // 3) Объединение с параллельным классом на том же предмете в то же время.
        let grade = load.classes.first().and_then(|c| self.ix.class(*c)).map(|c| c.grade);
        for (li, other) in self.tt.lessons.iter().enumerate() {
            if li == a.lesson_idx || other.slot != a.slot {
                continue;
            }
            let Some(ol) = self.ix.load(other.load) else {
                continue;
            };
            for (gi, og) in ol.groups.iter().enumerate() {
                if og.subject != group.subject || self.is_absent(og.teacher, a.date, a.slot.period) {
                    continue;
                }
                let og_grade = ol.classes.first().and_then(|c| self.ix.class(*c)).map(|c| c.grade);
                if og_grade != grade {
                    continue;
                }
                let r = other.rooms.get(gi).and_then(|r| self.ix.room(*r));
                let total = students + self.ix.students_in(ol, gi);
                let mut score = 45;
                let mut warnings = vec![];
                if let Some(r) = r {
                    if r.capacity < total {
                        score -= 50;
                        warnings.push(format!("в кабинете {} мест {}, будет {}", r.name, r.capacity, total));
                    }
                }
                out.push(Suggestion {
                    kind: ReplacementKind::Merge { with_lesson: other.id, group: gi },
                    room: r.map(|r| r.id),
                    score,
                    reasons: vec![format!("параллельный урок: {} у {} — объединить", self.ix.classes_label(&ol.classes), self.ix.teacher_name(og.teacher))],
                    warnings,
                });
            }
        }

        // 4) Снять урок.
        let mut edge = true;
        for c in &load.classes {
            let ps = self.class_periods(*c, a.date, reps);
            let first = ps.iter().min().copied();
            let last = ps.iter().max().copied();
            if first != Some(a.slot.period) && last != Some(a.slot.period) {
                edge = false;
            }
        }
        out.push(if edge {
            Suggestion {
                kind: ReplacementKind::Cancel,
                room: None,
                score: if policy.allow_cancel_edge { 15 } else { -10 },
                reasons: vec!["урок крайний — класс придёт позже или уйдёт раньше".into()],
                warnings: vec!["часы по программе нужно будет восполнить".into()],
            }
        } else {
            Suggestion {
                kind: ReplacementKind::Cancel,
                room: None,
                score: -60,
                reasons: vec![],
                warnings: vec!["урок в середине дня — у класса будет окно".into()],
            }
        });

        out.sort_by(|a, b| b.score.cmp(&a.score));
        out
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AutoPlan {
    pub replacements: Vec<Replacement>,
    /// Уроки, для которых не нашлось хорошего варианта (нужен человек).
    pub unresolved: Vec<(AffectedLesson, Vec<Suggestion>)>,
}

/// Автоматический подбор замен на интервал. Сначала обрабатываются уроки,
/// у которых меньше всего кандидатов, — так редкие специалисты не «съедаются»
/// лёгкими случаями.
pub fn auto_plan(ctx: &SubContext, existing: &[Replacement], from: NaiveDate, to: NaiveDate, policy: &SubPolicy, next_id: &mut Id) -> AutoPlan {
    let mut planned: Vec<Replacement> = vec![];
    let covered =
        |p: &[Replacement], a: &AffectedLesson| existing.iter().chain(p.iter()).any(|r| r.date == a.date && r.lesson == a.lesson_id && r.group == a.group);
    let mut todo: Vec<AffectedLesson> = ctx.affected(from, to).into_iter().filter(|a| !covered(&[], a)).collect();
    let mut unresolved = vec![];
    while !todo.is_empty() {
        let all: Vec<&Replacement> = existing.iter().chain(planned.iter()).collect();
        // самый «трудный» урок
        let (pos, sugg) = todo
            .iter()
            .enumerate()
            .map(|(i, a)| (i, ctx.suggest(a, &all, policy)))
            .min_by_key(|(_, s)| (s.iter().filter(|x| x.score >= policy.min_auto_score).count(), std::cmp::Reverse(s.first().map(|x| x.score).unwrap_or(0))))
            .unwrap();
        let a = todo.remove(pos);
        let pick = sugg.iter().find(|x| x.score >= policy.min_auto_score && (policy.allow_cancel_edge || x.kind != ReplacementKind::Cancel));
        match pick {
            Some(sg) => {
                let id = *next_id;
                *next_id += 1;
                planned.push(Replacement {
                    id,
                    date: a.date,
                    lesson: a.lesson_id,
                    group: a.group,
                    original_teacher: a.teacher,
                    kind: sg.kind.clone(),
                    room: {
                        let original = ctx.lesson(a.lesson_id).and_then(|l| l.rooms.get(a.group).copied());
                        if sg.room == original {
                            None
                        } else {
                            sg.room
                        }
                    },
                    note: sg.reasons.first().cloned().unwrap_or_default(),
                    confirmed: false,
                });
            }
            None => unresolved.push((a, sugg)),
        }
    }
    planned.sort_by_key(|r| (r.date, ctx.lesson(r.lesson).map(|l| l.slot.period), r.lesson));
    AutoPlan { replacements: planned, unresolved }
}

/// Проверка назначенных замен на конфликты и «совпадения».
pub fn check_replacements(ctx: &SubContext, reps: &[Replacement]) -> Vec<Issue> {
    let mut out = vec![];
    let ix = &ctx.ix;
    let mut seen: HashMap<(NaiveDate, Id, usize), Id> = HashMap::new();
    let mut sub_at: HashMap<(NaiveDate, Id, Slot), Id> = HashMap::new();
    let mut room_at: HashMap<(NaiveDate, Id, Slot), Id> = HashMap::new();
    for r in reps {
        let Some(l) = ctx.lesson(r.lesson) else {
            out.push(Issue {
                severity: Severity::Error,
                code: "rep_bad_lesson",
                message: format!("Замена #{}: урок #{} не найден в расписании", r.id, r.lesson),
                slot: None,
                lessons: vec![],
            });
            continue;
        };
        let date_s = r.date.format("%d.%m.%Y");
        if day_of(r.date, ctx.school.settings.days_per_week) != Some(l.slot.day) {
            out.push(Issue {
                severity: Severity::Error,
                code: "rep_wrong_day",
                message: format!("Замена #{} на {date_s}: урок стоит в другой день недели", r.id),
                slot: Some(l.slot),
                lessons: vec![l.id],
            });
        }
        if let Some(prev) = seen.insert((r.date, r.lesson, r.group), r.id) {
            out.push(Issue {
                severity: Severity::Error,
                code: "rep_duplicate",
                message: format!("Замены #{prev} и #{} на один и тот же урок {date_s}", r.id),
                slot: Some(l.slot),
                lessons: vec![l.id],
            });
        }
        if !ctx.is_absent(r.original_teacher, r.date, l.slot.period) {
            out.push(Issue {
                severity: Severity::Warning,
                code: "rep_no_absence",
                message: format!("Замена #{} {date_s}: {} не отмечен(а) отсутствующим", r.id, ix.teacher_name(r.original_teacher)),
                slot: Some(l.slot),
                lessons: vec![l.id],
            });
        }
        if let ReplacementKind::Substitute { teacher, .. } = r.kind {
            let others: Vec<&Replacement> = reps.iter().filter(|x| x.id != r.id).collect();
            if let Some(why) = ctx.busy(teacher, r.date, l.slot, &others) {
                out.push(Issue {
                    severity: Severity::Error,
                    code: "rep_teacher_busy",
                    message: format!("Замена #{} {date_s} {} урок: {} занят(а) — {why}", r.id, l.slot.period + 1, ix.teacher_name(teacher)),
                    slot: Some(l.slot),
                    lessons: vec![l.id],
                });
            }
            if let Some(prev) = sub_at.insert((r.date, teacher, l.slot), r.id) {
                out.push(Issue {
                    severity: Severity::Error,
                    code: "rep_teacher_double",
                    message: format!("{} на двух заменах одновременно (#{prev} и #{}) {date_s}", ix.teacher_name(teacher), r.id),
                    slot: Some(l.slot),
                    lessons: vec![l.id],
                });
            }
        }
        if let Some(room) = r.room {
            let busy_base = ctx.occ.room_load(room, l.slot) > 0;
            if busy_base {
                out.push(Issue {
                    severity: Severity::Warning,
                    code: "rep_room_busy",
                    message: format!("Замена #{} {date_s}: кабинет {} по расписанию занят — проверьте", r.id, ix.room_name(room)),
                    slot: Some(l.slot),
                    lessons: vec![l.id],
                });
            }
            if let Some(prev) = room_at.insert((r.date, room, l.slot), r.id) {
                out.push(Issue {
                    severity: Severity::Error,
                    code: "rep_room_double",
                    message: format!("Кабинет {} отдан двум заменам (#{prev} и #{})", ix.room_name(room), r.id),
                    slot: Some(l.slot),
                    lessons: vec![l.id],
                });
            }
        }
    }
    out.sort_by_key(|i| i.severity);
    out
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TeacherSubStats {
    pub teacher: Id,
    /// Сколько уроков провёл на заменах.
    pub substituted: u32,
    /// Сколько его уроков заменили/сняли.
    pub replaced: u32,
    pub paid_hours: u32,
}

/// Табель замен за период (для оплаты и учёта часов).
pub fn stats(ctx: &SubContext, reps: &[Replacement], from: NaiveDate, to: NaiveDate) -> Vec<TeacherSubStats> {
    let mut m: HashMap<Id, TeacherSubStats> = HashMap::new();
    for r in reps.iter().filter(|r| r.date >= from && r.date <= to) {
        m.entry(r.original_teacher).or_insert_with(|| TeacherSubStats { teacher: r.original_teacher, ..Default::default() }).replaced += 1;
        if let ReplacementKind::Substitute { teacher, .. } = r.kind {
            let e = m.entry(teacher).or_insert_with(|| TeacherSubStats { teacher, ..Default::default() });
            e.substituted += 1;
            e.paid_hours += 1;
        }
    }
    let mut v: Vec<_> = m.into_values().collect();
    v.sort_by_key(|s| ctx.ix.teacher_name(s.teacher));
    v
}
