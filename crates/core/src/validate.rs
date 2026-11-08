// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Анализ расписания: жёсткие конфликты («совпадения») и мягкие нарушения.
//! Работает с любым расписанием — сгенерированным или отредактированным вручную.

use crate::grid::{class_weeks, teacher_weeks, Occupancy};
use crate::model::*;
use crate::score::{class_week, teacher_week, Stats};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub slot: Option<Slot>,
    pub lessons: Vec<Id>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub issues: Vec<Issue>,
    pub stats: Stats,
    pub score: i64,
}

impl Report {
    pub fn errors(&self) -> usize {
        self.issues.iter().filter(|i| i.severity == Severity::Error).count()
    }
}

fn slot_label(s: Slot) -> String {
    format!("{} {} урок", DAY_NAMES[s.day as usize % 7], s.period + 1)
}

/// Проверка исходных данных до генерации (аналог проверки в «Мастере поиска»).
pub fn precheck(school: &School) -> Vec<Issue> {
    let ix = Index::new(school);
    let s = &school.settings;
    let mut out = vec![];
    let cells =
        |a: &Availability| (0..s.days_per_week).flat_map(|d| (0..s.periods_per_day).map(move |p| Slot::new(d, p))).filter(|sl| a.get(*sl) != Avail::No).count();
    let mut class_hours: HashMap<Id, u32> = HashMap::new();
    let mut teacher_hours: HashMap<Id, u32> = HashMap::new();
    for l in &school.loads {
        if l.groups.is_empty() {
            out.push(Issue {
                severity: Severity::Error,
                code: "load_no_groups",
                message: format!("Нагрузка #{} без учителя", l.id),
                slot: None,
                lessons: vec![],
            });
        }
        for c in &l.classes {
            if ix.class(*c).is_none() {
                out.push(err("bad_ref", format!("Нагрузка #{}: нет класса #{c}", l.id)));
            }
            *class_hours.entry(*c).or_default() += l.hours_per_week as u32;
        }
        for g in &l.groups {
            if ix.teacher(g.teacher).is_none() {
                out.push(err("bad_ref", format!("Нагрузка #{}: нет учителя #{}", l.id, g.teacher)));
            }
            if ix.subject(g.subject).is_none() {
                out.push(err("bad_ref", format!("Нагрузка #{}: нет предмета #{}", l.id, g.subject)));
            }
            *teacher_hours.entry(g.teacher).or_default() += l.hours_per_week as u32;
        }
        if l.pairing == Pairing::NoDouble && l.hours_per_week > s.days_per_week {
            out.push(err(
                "pairing",
                format!(
                    "Нагрузка #{} ({}): {} ч при запрете двух уроков в день, а дней всего {}",
                    l.id,
                    ix.classes_label(&l.classes),
                    l.hours_per_week,
                    s.days_per_week
                ),
            ));
        }
    }
    for c in &school.classes {
        let h = class_hours.get(&c.id).copied().unwrap_or(0);
        let avail = cells(&c.availability) as u32;
        if h > avail {
            out.push(err("class_hours", format!("Класс {} может учиться {avail} ч в неделю при нагрузке {h} ч", c.name)));
        }
        if h > c.max_per_day as u32 * s.days_per_week as u32 {
            out.push(err("class_max", format!("Класс {}: нагрузка {h} ч не помещается при максимуме {} уроков в день", c.name, c.max_per_day)));
        }
    }
    for t in &school.teachers {
        let h = teacher_hours.get(&t.id).copied().unwrap_or(0);
        let avail = cells(&t.availability) as u32;
        if h > avail {
            out.push(err("teacher_hours", format!("Учитель {} может работать {avail} ч при нагрузке {h} ч", t.short_name())));
        }
    }
    out
}

fn err(code: &'static str, message: String) -> Issue {
    Issue { severity: Severity::Error, code, message, slot: None, lessons: vec![] }
}

pub fn analyze(school: &School, tt: &Timetable) -> Report {
    let ix = Index::new(school);
    let s = &school.settings;
    let occ = Occupancy::build(school, tt);
    let mut issues = vec![];
    let lid = |i: usize| tt.lessons[i].id;

    for l in &tt.lessons {
        let Some(load) = ix.load(l.load) else {
            issues.push(Issue {
                severity: Severity::Error,
                code: "bad_load",
                message: format!("Урок #{} ссылается на несуществующую нагрузку #{}", l.id, l.load),
                slot: Some(l.slot),
                lessons: vec![l.id],
            });
            continue;
        };
        if l.slot.day >= s.days_per_week || l.slot.period >= s.periods_per_day {
            issues.push(Issue {
                severity: Severity::Error,
                code: "out_of_grid",
                message: format!("Урок #{} вне сетки", l.id),
                slot: Some(l.slot),
                lessons: vec![l.id],
            });
        }
        if l.rooms.len() != load.groups.len() {
            issues.push(Issue {
                severity: Severity::Warning,
                code: "no_room",
                message: format!("{}: не назначен кабинет", ix.classes_label(&load.classes)),
                slot: Some(l.slot),
                lessons: vec![l.id],
            });
        }
        for (gi, g) in load.groups.iter().enumerate() {
            if let Some(t) = ix.teacher(g.teacher) {
                match t.availability.get(l.slot) {
                    Avail::No => issues.push(Issue {
                        severity: Severity::Error,
                        code: "teacher_unavailable",
                        message: format!("{} не работает в {}", t.short_name(), slot_label(l.slot)),
                        slot: Some(l.slot),
                        lessons: vec![l.id],
                    }),
                    Avail::IfNeeded => issues.push(Issue {
                        severity: Severity::Info,
                        code: "teacher_if_needed",
                        message: format!("{}: урок в нежелательное время ({})", t.short_name(), slot_label(l.slot)),
                        slot: Some(l.slot),
                        lessons: vec![l.id],
                    }),
                    Avail::Yes => {}
                }
            }
            if let Some(room) = l.rooms.get(gi).and_then(|r| ix.room(*r)) {
                let need = ix.students_in(load, gi);
                if need > room.capacity {
                    issues.push(Issue {
                        severity: Severity::Warning,
                        code: "room_capacity",
                        message: format!("Кабинет {} на {} мест, а в группе {} чел. ({})", room.name, room.capacity, need, ix.classes_label(&load.classes)),
                        slot: Some(l.slot),
                        lessons: vec![l.id],
                    });
                }
                if room.availability.get(l.slot) == Avail::No {
                    issues.push(Issue {
                        severity: Severity::Error,
                        code: "room_unavailable",
                        message: format!("Кабинет {} закрыт в {}", room.name, slot_label(l.slot)),
                        slot: Some(l.slot),
                        lessons: vec![l.id],
                    });
                }
            }
        }
        for c in &load.classes {
            if let Some(cl) = ix.class(*c) {
                if cl.availability.get(l.slot) == Avail::No {
                    issues.push(Issue {
                        severity: Severity::Error,
                        code: "class_unavailable",
                        message: format!("Класс {} не учится в {}", cl.name, slot_label(l.slot)),
                        slot: Some(l.slot),
                        lessons: vec![l.id],
                    });
                }
            }
        }
    }

    // Совпадения: один учитель / класс / кабинет в двух местах одновременно.
    let mut keys: Vec<_> = occ.teacher.iter().collect();
    keys.sort_by_key(|((t, s), _)| (*s, *t));
    for ((t, slot), refs) in keys {
        let mut ls: Vec<usize> = refs.iter().map(|r| r.lesson).collect();
        ls.dedup();
        if ls.len() > 1 {
            issues.push(Issue {
                severity: Severity::Error,
                code: "teacher_clash",
                message: format!("{} одновременно на {} уроках ({})", ix.teacher_name(*t), ls.len(), slot_label(*slot)),
                slot: Some(*slot),
                lessons: ls.iter().map(|i| lid(*i)).collect(),
            });
        }
    }
    let mut keys: Vec<_> = occ.class.iter().collect();
    keys.sort_by_key(|((c, s), _)| (*s, *c));
    for ((c, slot), ls) in keys {
        if ls.len() > 1 {
            issues.push(Issue {
                severity: Severity::Error,
                code: "class_clash",
                message: format!("У класса {} два урока в {}", ix.class_name(*c), slot_label(*slot)),
                slot: Some(*slot),
                lessons: ls.iter().map(|i| lid(*i)).collect(),
            });
        }
    }
    let mut keys: Vec<_> = occ.room.iter().collect();
    keys.sort_by_key(|((r, s), _)| (*s, *r));
    for ((r, slot), refs) in keys {
        let cap = ix.room(*r).map(|r| r.parallel_groups as usize).unwrap_or(1);
        let mut ls: Vec<usize> = refs.iter().map(|r| r.lesson).collect();
        ls.dedup();
        if refs.len() > cap && ls.len() > 1 {
            issues.push(Issue {
                severity: Severity::Error,
                code: "room_clash",
                message: format!("Кабинет {} занят {} группами (можно {}) в {}", ix.room_name(*r), refs.len(), cap, slot_label(*slot)),
                slot: Some(*slot),
                lessons: ls.iter().map(|i| lid(*i)).collect(),
            });
        }
    }

    // Часы и спаривание по нагрузкам.
    let mut per_load: HashMap<Id, Vec<Slot>> = HashMap::new();
    for l in &tt.lessons {
        per_load.entry(l.load).or_default().push(l.slot);
    }
    for load in &school.loads {
        let slots = per_load.get(&load.id).cloned().unwrap_or_default();
        let placed = slots.len() as i64;
        let need = load.hours_per_week as i64;
        let label = format!("{} — {}", ix.classes_label(&load.classes), load.groups.iter().map(|g| ix.subject_name(g.subject)).collect::<Vec<_>>().join("/"));
        if placed < need {
            issues.push(Issue {
                severity: Severity::Error,
                code: "unplaced",
                message: format!("{label}: не расставлено {} ч из {need}", need - placed),
                slot: None,
                lessons: vec![],
            });
        } else if placed > need {
            issues.push(Issue {
                severity: Severity::Warning,
                code: "overplaced",
                message: format!("{label}: поставлено {placed} ч вместо {need}"),
                slot: None,
                lessons: vec![],
            });
        }
        let mut by_day: HashMap<u8, Vec<u8>> = HashMap::new();
        for sl in &slots {
            by_day.entry(sl.day).or_default().push(sl.period);
        }
        for (d, mut ps) in by_day {
            ps.sort();
            let bad = match load.pairing {
                Pairing::Any => ps.len() > 2,
                Pairing::NoDouble => ps.len() > 1,
                Pairing::Double => ps.len() % 2 != 0 || ps.chunks(2).any(|c| c.len() == 2 && c[1] != c[0] + 1),
            };
            if bad {
                issues.push(Issue {
                    severity: Severity::Warning,
                    code: "pairing",
                    message: format!("{label}: нарушено спаривание в {}", DAY_NAMES_FULL[d as usize % 7]),
                    slot: None,
                    lessons: vec![],
                });
            }
        }
    }

    // Мягкие требования.
    let mut stats = Stats::default();
    let cw = class_weeks(school, tt);
    for c in &school.classes {
        if let Some(w) = cw.get(&c.id) {
            let st = class_week(w, c, s);
            if st.class_building_changes > 0 {
                issues.push(Issue {
                    severity: Severity::Error,
                    code: "class_building",
                    message: format!("Класс {} переходит между корпусами в течение дня", c.name),
                    slot: None,
                    lessons: vec![],
                });
            }
            stats.add(&st);
        }
    }
    let tw = teacher_weeks(school, tt);
    for t in &school.teachers {
        if let Some(w) = tw.get(&t.id) {
            let st = teacher_week(w, t, s);
            if st.teacher_impossible_transitions > 0 {
                issues.push(Issue {
                    severity: Severity::Error,
                    code: "teacher_building",
                    message: format!("{}: уроки подряд в разных корпусах без времени на переход", t.short_name()),
                    slot: None,
                    lessons: vec![],
                });
            }
            stats.add(&st);
        }
    }
    stats.unplaced = school.loads.iter().map(|l| (l.hours_per_week as i64 - per_load.get(&l.id).map(|v| v.len()).unwrap_or(0) as i64).max(0)).sum();
    issues.sort_by_key(|i| i.severity);
    let score = stats.total(&s.weights);
    Report { issues, stats, score }
}
