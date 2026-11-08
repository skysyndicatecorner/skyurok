// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Индекс занятости по ячейкам сетки и построение недельных представлений.

use crate::model::*;
use crate::score::{ClassCell, TeacherCell};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LessonRef {
    /// Индекс в `Timetable::lessons`.
    pub lesson: usize,
    pub group: usize,
}

#[derive(Default)]
pub struct Occupancy {
    pub teacher: HashMap<(Id, Slot), Vec<LessonRef>>,
    pub class: HashMap<(Id, Slot), Vec<usize>>,
    pub room: HashMap<(Id, Slot), Vec<LessonRef>>,
}

impl Occupancy {
    pub fn build(school: &School, tt: &Timetable) -> Self {
        let ix = Index::new(school);
        let mut o = Occupancy::default();
        for (li, l) in tt.lessons.iter().enumerate() {
            let Some(load) = ix.load(l.load) else {
                continue;
            };
            for c in &load.classes {
                o.class.entry((*c, l.slot)).or_default().push(li);
            }
            for (gi, g) in load.groups.iter().enumerate() {
                let r = LessonRef { lesson: li, group: gi };
                o.teacher.entry((g.teacher, l.slot)).or_default().push(r);
                if let Some(room) = l.rooms.get(gi) {
                    o.room.entry((*room, l.slot)).or_default().push(r);
                }
            }
        }
        o
    }

    pub fn teacher_busy(&self, t: Id, s: Slot) -> bool {
        self.teacher.get(&(t, s)).is_some_and(|v| !v.is_empty())
    }
    pub fn room_load(&self, r: Id, s: Slot) -> usize {
        self.room.get(&(r, s)).map(|v| v.len()).unwrap_or(0)
    }
}

pub fn class_cell(ix: &Index, load: &Load, rooms: &[Id], grade: u8) -> ClassCell {
    let difficulty = load.groups.iter().filter_map(|g| ix.subject(g.subject)).map(|s| s.difficulty_for_grade(grade)).max().unwrap_or(0);
    let building = rooms.first().and_then(|r| ix.room(*r)).map(|r| r.building);
    ClassCell { difficulty, position: load.position, building }
}

pub type Week<T> = Vec<Vec<Option<T>>>;

pub fn empty_week<T: Clone>(s: &Settings) -> Week<T> {
    vec![vec![None; s.periods_per_day as usize]; s.days_per_week as usize]
}

fn in_grid(s: &Settings, slot: Slot) -> bool {
    slot.day < s.days_per_week && slot.period < s.periods_per_day
}

pub fn class_weeks(school: &School, tt: &Timetable) -> HashMap<Id, Week<ClassCell>> {
    let ix = Index::new(school);
    let mut m: HashMap<Id, Week<ClassCell>> = school.classes.iter().map(|c| (c.id, empty_week(&school.settings))).collect();
    for l in &tt.lessons {
        if !in_grid(&school.settings, l.slot) {
            continue;
        }
        let Some(load) = ix.load(l.load) else {
            continue;
        };
        for c in &load.classes {
            let Some(cl) = ix.class(*c) else { continue };
            let cell = class_cell(&ix, load, &l.rooms, cl.grade);
            if let Some(w) = m.get_mut(c) {
                w[l.slot.day as usize][l.slot.period as usize] = Some(cell);
            }
        }
    }
    m
}

pub fn teacher_weeks(school: &School, tt: &Timetable) -> HashMap<Id, Week<TeacherCell>> {
    let ix = Index::new(school);
    let mut m: HashMap<Id, Week<TeacherCell>> = school.teachers.iter().map(|t| (t.id, empty_week(&school.settings))).collect();
    for l in &tt.lessons {
        if !in_grid(&school.settings, l.slot) {
            continue;
        }
        let Some(load) = ix.load(l.load) else {
            continue;
        };
        for (gi, g) in load.groups.iter().enumerate() {
            let building = l.rooms.get(gi).and_then(|r| ix.room(*r)).map(|r| r.building);
            let if_needed = ix.teacher(g.teacher).is_some_and(|t| t.availability.get(l.slot) == Avail::IfNeeded);
            if let Some(w) = m.get_mut(&g.teacher) {
                w[l.slot.day as usize][l.slot.period as usize] = Some(TeacherCell { building, if_needed });
            }
        }
    }
    m
}
