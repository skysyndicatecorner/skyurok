// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Версии расписания и перенос замен на новое расписание.
//!
//! При каждом составлении уроки получают новые номера. Замена помнит не только
//! номер урока, но и якорь — нагрузку и номер урока в дне. По якорю она находит
//! свой урок в новом расписании.

use crate::model::*;
use chrono::NaiveDate;
use std::collections::HashMap;

/// Что произойдёт с заменами при переходе на новое расписание.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RebaseReport {
    /// Урок остался на месте — замена переносится как есть.
    pub kept: usize,
    /// Урок переехал внутри того же дня — замена перенесена и помечена «пересмотреть».
    pub shifted: usize,
    /// Прошедшие даты — замена остаётся в истории и табеле.
    pub archived: usize,
    /// В этот день урока больше нет — замена удаляется.
    pub removed: usize,
    /// Из удалённых — подтверждённые людьми.
    pub removed_confirmed: usize,
}

impl RebaseReport {
    pub fn touched(&self) -> usize {
        self.shifted + self.removed
    }
}

/// Перенести замены проекта на расписание `new_tt`.
/// `today` — замены раньше этой даты считаются историей и не трогаются.
pub fn rebase_replacements(p: &Project, new_tt: &Timetable, today: NaiveDate) -> (Vec<Replacement>, RebaseReport) {
    let days = p.school.settings.days_per_week;
    let old: HashMap<Id, &Lesson> = p.timetable.lessons.iter().map(|l| (l.id, l)).collect();
    let mut by_load_day: HashMap<(Id, u8), Vec<&Lesson>> = HashMap::new();
    for l in &new_tt.lessons {
        by_load_day.entry((l.load, l.slot.day)).or_default().push(l);
    }
    let mut out = Vec::with_capacity(p.replacements.len());
    let mut rep = RebaseReport::default();
    for r in &p.replacements {
        let mut r = r.clone();
        let anchor = r.anchor.or_else(|| old.get(&r.lesson).map(|l| Anchor { load: l.load, period: l.slot.period }));
        let (Some(a), Some(day)) = (anchor, day_of(r.date, days)) else {
            // Нечего сопоставлять — оставляем как было.
            out.push(r);
            rep.kept += 1;
            continue;
        };
        r.anchor = Some(a);
        let same_day = by_load_day.get(&(a.load, day));
        if let Some(l) = same_day.and_then(|v| v.iter().find(|l| l.slot.period == a.period)) {
            r.lesson = l.id;
            out.push(r);
            rep.kept += 1;
            continue;
        }
        if r.date < today {
            // История: урока на этом месте больше нет, но замена была.
            r.lesson = 0;
            out.push(r);
            rep.archived += 1;
            continue;
        }
        match same_day.and_then(|v| v.iter().min_by_key(|l| (l.slot.period as i32 - a.period as i32).abs())) {
            Some(l) => {
                r.lesson = l.id;
                r.anchor = Some(Anchor { load: a.load, period: l.slot.period });
                r.stale = true;
                out.push(r);
                rep.shifted += 1;
            }
            None => {
                rep.removed += 1;
                if r.confirmed {
                    rep.removed_confirmed += 1;
                }
            }
        }
    }
    (out, rep)
}

/// Применить новое расписание: сохранить старое как версию и перенести замены.
pub fn apply_timetable(p: &mut Project, new_tt: Timetable, today: NaiveDate, now: &str, reason: &str) -> RebaseReport {
    if !p.timetable.lessons.is_empty() {
        save_version(p, reason, now, true);
    }
    let (reps, report) = rebase_replacements(p, &new_tt, today);
    p.timetable = new_tt;
    p.replacements = reps;
    report
}

/// Сколько автоматических версий хранить (сохранённые вручную не удаляются).
pub const AUTO_VERSIONS_KEEP: usize = 10;

pub fn save_version(p: &mut Project, name: &str, now: &str, auto: bool) -> Id {
    let id = p.versions.iter().map(|v| v.id).max().unwrap_or(0) + 1;
    p.versions.push(TimetableVersion { id, name: name.to_string(), created: now.to_string(), auto, timetable: p.timetable.clone() });
    let autos: Vec<Id> = p.versions.iter().filter(|v| v.auto).map(|v| v.id).collect();
    if autos.len() > AUTO_VERSIONS_KEEP {
        let drop: Vec<Id> = autos[..autos.len() - AUTO_VERSIONS_KEEP].to_vec();
        p.versions.retain(|v| !drop.contains(&v.id));
    }
    id
}

/// Вернуть сохранённую версию. Текущее расписание перед этим тоже сохраняется.
pub fn restore_version(p: &mut Project, id: Id, today: NaiveDate, now: &str) -> Option<RebaseReport> {
    let tt = p.versions.iter().find(|v| v.id == id)?.timetable.clone();
    Some(apply_timetable(p, tt, today, now, "Перед восстановлением версии"))
}
