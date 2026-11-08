// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Ручная работа с сеткой: можно ли поставить урок в ячейку (и почему нет),
//! перенос, постановка нерасставленных часов, закрепление.

use crate::grid::Occupancy;
use crate::model::*;
use std::collections::HashMap;

/// Сколько часов нагрузки ещё не стоит в расписании.
pub fn unplaced(school: &School, tt: &Timetable) -> Vec<(Id, u8)> {
    let mut placed: HashMap<Id, u8> = HashMap::new();
    for l in &tt.lessons {
        *placed.entry(l.load).or_default() += 1;
    }
    school
        .loads
        .iter()
        .filter_map(|l| {
            let p = placed.get(&l.id).copied().unwrap_or(0);
            (p < l.hours_per_week).then(|| (l.id, l.hours_per_week - p))
        })
        .collect()
}

pub struct Planner<'a> {
    pub school: &'a School,
    pub tt: &'a Timetable,
    pub ix: Index<'a>,
    occ: Occupancy,
}

impl<'a> Planner<'a> {
    pub fn new(school: &'a School, tt: &'a Timetable) -> Self {
        Self { school, tt, ix: Index::new(school), occ: Occupancy::build(school, tt) }
    }

    fn lesson_label(&self, li: usize) -> String {
        let l = &self.tt.lessons[li];
        self.ix
            .load(l.load)
            .map(|ld| {
                format!(
                    "{} {}",
                    self.ix.classes_label(&ld.classes),
                    ld.groups.iter().map(|g| self.ix.subject(g.subject).map(|s| s.short.clone()).unwrap_or_default()).collect::<Vec<_>>().join("/")
                )
            })
            .unwrap_or_default()
    }

    /// Можно ли поставить урок нагрузки `load` в `slot`. `moving` — урок, который
    /// переносится (он не мешает сам себе). Ok — кабинеты по группам, Err — причины.
    pub fn check(&self, load_id: Id, slot: Slot, moving: Option<Id>) -> Result<Vec<Id>, Vec<String>> {
        let s = &self.school.settings;
        let Some(load) = self.ix.load(load_id) else { return Err(vec!["нагрузка не найдена".into()]) };
        if slot.day >= s.days_per_week || slot.period >= s.periods_per_day {
            return Err(vec!["вне сетки".into()]);
        }
        let moving_idx = moving.and_then(|m| self.tt.lessons.iter().position(|l| l.id == m));
        let mut why = vec![];
        for c in &load.classes {
            let Some(cl) = self.ix.class(*c) else { continue };
            if cl.availability.get(slot) == Avail::No {
                why.push(format!("класс {} не учится в это время", cl.name));
            }
            if let Some(ls) = self.occ.class.get(&(*c, slot)) {
                for li in ls.iter().filter(|li| Some(**li) != moving_idx) {
                    why.push(format!("у класса {} уже стоит {}", cl.name, self.lesson_label(*li)));
                }
            }
        }
        for g in &load.groups {
            let Some(t) = self.ix.teacher(g.teacher) else { continue };
            if t.availability.get(slot) == Avail::No {
                why.push(format!("{} не работает в это время", t.short_name()));
            }
            if let Some(refs) = self.occ.teacher.get(&(g.teacher, slot)) {
                for r in refs.iter().filter(|r| Some(r.lesson) != moving_idx) {
                    why.push(format!("{} ведёт {}", t.short_name(), self.lesson_label(r.lesson)));
                }
            }
        }
        // спаривание: сколько уроков нагрузки уже в этот день
        let same_day = self.tt.lessons.iter().enumerate().filter(|(i, l)| l.load == load_id && l.slot.day == slot.day && Some(*i) != moving_idx).count();
        let max = match load.pairing {
            Pairing::NoDouble => 1,
            _ => 2,
        };
        if same_day >= max {
            why.push(format!("уже {same_day} ур. этой нагрузки в этот день"));
        }
        // корпус класса в этот день
        let mut building = load.classes.iter().filter_map(|c| self.ix.class(*c)).find_map(|c| c.building);
        if building.is_none() {
            for c in &load.classes {
                for p in 0..s.periods_per_day {
                    if let Some(ls) = self.occ.class.get(&(*c, Slot::new(slot.day, p))) {
                        for li in ls.iter().filter(|li| Some(**li) != moving_idx) {
                            if let Some(r) = self.tt.lessons[*li].rooms.first().and_then(|r| self.ix.room(*r)) {
                                building.get_or_insert(r.building);
                            }
                        }
                    }
                }
            }
        }
        // кабинеты
        let current: Option<&Lesson> = moving_idx.map(|i| &self.tt.lessons[i]);
        let mut rooms: Vec<Id> = vec![];
        for (gi, g) in load.groups.iter().enumerate() {
            let need = self.ix.students_in(load, gi);
            let mut cands: Vec<Id> = vec![];
            if let Some(r) = current.and_then(|l| l.rooms.get(gi)) {
                cands.push(*r);
            }
            cands.extend(g.rooms.iter().copied());
            if g.rooms.is_empty() {
                if let Some(h) = self.ix.teacher(g.teacher).and_then(|t| t.home_room) {
                    cands.push(h);
                }
                let mut rest: Vec<&Room> =
                    self.school.rooms.iter().filter(|r| r.capacity >= need && r.tags.is_empty() && building.is_none_or(|b| r.building == b)).collect();
                rest.sort_by_key(|r| r.capacity);
                cands.extend(rest.iter().map(|r| r.id));
            }
            let found = cands.into_iter().find(|rid| {
                let Some(room) = self.ix.room(*rid) else { return false };
                if building.is_some_and(|b| b != room.building) || room.availability.get(slot) == Avail::No {
                    return false;
                }
                let used = self.occ.room.get(&(*rid, slot)).map(|v| v.iter().filter(|r| Some(r.lesson) != moving_idx).count()).unwrap_or(0)
                    + rooms.iter().filter(|x| *x == rid).count();
                used < room.parallel_groups as usize
            });
            match found {
                Some(r) => rooms.push(r),
                None => why.push(format!(
                    "нет свободного кабинета для {}{}",
                    self.ix.subject_name(g.subject),
                    if g.label.is_empty() { String::new() } else { format!(" ({})", g.label) }
                )),
            }
        }
        if why.is_empty() {
            Ok(rooms)
        } else {
            why.dedup();
            Err(why)
        }
    }

    /// Карта допустимости по всей сетке (для подсветки при перетаскивании).
    pub fn map(&self, load_id: Id, moving: Option<Id>) -> HashMap<Slot, Result<Vec<Id>, Vec<String>>> {
        let s = &self.school.settings;
        let mut m = HashMap::new();
        for d in 0..s.days_per_week {
            for p in 0..s.periods_per_day {
                let sl = Slot::new(d, p);
                m.insert(sl, self.check(load_id, sl, moving));
            }
        }
        m
    }
}

pub fn next_lesson_id(tt: &Timetable) -> Id {
    tt.lessons.iter().map(|l| l.id).max().unwrap_or(0) + 1
}

/// Перенести урок. Ошибка — список причин, расписание не меняется.
pub fn move_lesson(school: &School, tt: &mut Timetable, lesson: Id, to: Slot) -> Result<(), Vec<String>> {
    let load = tt.lessons.iter().find(|l| l.id == lesson).map(|l| l.load).ok_or(vec!["урок не найден".to_string()])?;
    let rooms = Planner::new(school, tt).check(load, to, Some(lesson))?;
    if let Some(l) = tt.lessons.iter_mut().find(|l| l.id == lesson) {
        l.slot = to;
        l.rooms = rooms;
    }
    Ok(())
}

/// Поменять два урока местами (с подбором кабинетов). Ошибка — причины.
pub fn swap_lessons(school: &School, tt: &mut Timetable, a: Id, b: Id) -> Result<(), Vec<String>> {
    let la = tt.lessons.iter().find(|l| l.id == a).cloned().ok_or(vec!["урок не найден".to_string()])?;
    let lb = tt.lessons.iter().find(|l| l.id == b).cloned().ok_or(vec!["урок не найден".to_string()])?;
    let mut work = tt.clone();
    // убираем B, переносим A на место B
    work.lessons.retain(|l| l.id != b);
    let rooms_a = Planner::new(school, &work)
        .check(la.load, lb.slot, Some(a))
        .map_err(|w| w.into_iter().map(|x| format!("{}: {x}", lesson_title(school, &la))).collect::<Vec<_>>())?;
    if let Some(l) = work.lessons.iter_mut().find(|l| l.id == a) {
        l.slot = lb.slot;
        l.rooms = rooms_a;
    }
    // возвращаем B на место A
    work.lessons.push(Lesson { slot: la.slot, ..lb.clone() });
    let rooms_b = Planner::new(school, &work)
        .check(lb.load, la.slot, Some(b))
        .map_err(|w| w.into_iter().map(|x| format!("{}: {x}", lesson_title(school, &lb))).collect::<Vec<_>>())?;
    if let Some(l) = work.lessons.iter_mut().find(|l| l.id == b) {
        l.rooms = rooms_b;
    }
    work.lessons.sort_by_key(|l| (l.slot, l.load));
    *tt = work;
    Ok(())
}

fn lesson_title(school: &School, l: &Lesson) -> String {
    let ix = Index::new(school);
    ix.load(l.load)
        .map(|ld| {
            format!(
                "{} {}",
                ix.classes_label(&ld.classes),
                ld.groups.iter().map(|g| ix.subject(g.subject).map(|s| s.short.clone()).unwrap_or_default()).collect::<Vec<_>>().join("/")
            )
        })
        .unwrap_or_default()
}

/// Поставить ещё один час нагрузки.
pub fn place(school: &School, tt: &mut Timetable, load: Id, at: Slot) -> Result<Id, Vec<String>> {
    let rooms = Planner::new(school, tt).check(load, at, None)?;
    let id = next_lesson_id(tt);
    tt.lessons.push(Lesson { id, load, slot: at, rooms, pinned: false });
    Ok(id)
}

/// Поставить, игнорируя конфликты (ручной режим «всё равно поставить»).
pub fn place_force(tt: &mut Timetable, load: Id, at: Slot, rooms: Vec<Id>) -> Id {
    let id = next_lesson_id(tt);
    tt.lessons.push(Lesson { id, load, slot: at, rooms, pinned: false });
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::demo_school;

    #[test]
    fn place_and_move() {
        let s = demo_school();
        let mut tt = Timetable::default();
        let load = s.loads[0].id;
        let id = place(&s, &mut tt, load, Slot::new(0, 0)).unwrap();
        // второй урок той же нагрузки в тот же слот — нельзя (класс занят)
        assert!(place(&s, &mut tt, load, Slot::new(0, 0)).is_err());
        move_lesson(&s, &mut tt, id, Slot::new(1, 2)).unwrap();
        assert_eq!(tt.lessons[0].slot, Slot::new(1, 2));
        assert_eq!(unplaced(&s, &tt).iter().find(|x| x.0 == load).unwrap().1, s.loads[0].hours_per_week - 1);
    }

    #[test]
    fn swap_two_lessons_of_one_class() {
        let s = demo_school();
        let mut tt = Timetable::default();
        let a = place(&s, &mut tt, s.loads[0].id, Slot::new(0, 0)).unwrap();
        let b = place(&s, &mut tt, s.loads[1].id, Slot::new(0, 1)).unwrap();
        swap_lessons(&s, &mut tt, a, b).unwrap();
        assert_eq!(tt.lessons.iter().find(|l| l.id == a).unwrap().slot, Slot::new(0, 1));
        assert_eq!(tt.lessons.iter().find(|l| l.id == b).unwrap().slot, Slot::new(0, 0));
    }
}
