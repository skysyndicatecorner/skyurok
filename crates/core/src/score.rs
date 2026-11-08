// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Мягкие требования (штрафы). Чистые функции над недельной сеткой одного
//! класса или одного учителя — их используют и генератор, и анализатор.

use crate::model::{Class, Id, PositionReq, Settings, Teacher, Weights};
use serde::Serialize;

#[derive(Debug, Clone, Copy)]
pub struct ClassCell {
    pub difficulty: u8,
    pub position: PositionReq,
    pub building: Option<Id>,
}

#[derive(Debug, Clone, Copy)]
pub struct TeacherCell {
    pub building: Option<Id>,
    pub if_needed: bool,
}

/// Счётчики нарушений. Итог = сумма счётчик × вес.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Stats {
    pub unplaced: i64,
    pub class_windows: i64,
    pub class_over_max: i64,
    pub class_under_min: i64,
    pub class_building_changes: i64,
    pub teacher_single_windows: i64,
    pub teacher_multi_windows: i64,
    pub teacher_one_lesson_days: i64,
    pub teacher_building_changes: i64,
    /// Уроки подряд в разных корпусах без перемены, достаточной для перехода.
    pub teacher_impossible_transitions: i64,
    pub teacher_if_needed: i64,
    pub teacher_methodical_missing: i64,
    pub position_violations: i64,
    pub hygiene_peak: i64,
    pub hygiene_hardest_at_edge: i64,
}

impl Stats {
    pub fn add(&mut self, o: &Stats) {
        self.unplaced += o.unplaced;
        self.class_windows += o.class_windows;
        self.class_over_max += o.class_over_max;
        self.class_under_min += o.class_under_min;
        self.class_building_changes += o.class_building_changes;
        self.teacher_single_windows += o.teacher_single_windows;
        self.teacher_multi_windows += o.teacher_multi_windows;
        self.teacher_one_lesson_days += o.teacher_one_lesson_days;
        self.teacher_building_changes += o.teacher_building_changes;
        self.teacher_impossible_transitions += o.teacher_impossible_transitions;
        self.teacher_if_needed += o.teacher_if_needed;
        self.teacher_methodical_missing += o.teacher_methodical_missing;
        self.position_violations += o.position_violations;
        self.hygiene_peak += o.hygiene_peak;
        self.hygiene_hardest_at_edge += o.hygiene_hardest_at_edge;
    }

    pub fn total(&self, w: &Weights) -> i64 {
        self.unplaced * w.unplaced
            + self.class_windows * w.class_window
            + self.class_over_max * w.class_over_max
            + self.class_under_min * w.class_under_min
            // смена корпуса классом — фактически жёсткое требование
            + self.class_building_changes * w.unplaced
            + self.teacher_single_windows * w.teacher_single_window
            + self.teacher_multi_windows * w.teacher_multi_window
            + self.teacher_one_lesson_days * w.teacher_one_lesson_day
            + self.teacher_building_changes * w.teacher_building_change
            + self.teacher_impossible_transitions * w.unplaced
            + self.teacher_if_needed * w.teacher_if_needed_slot
            + self.teacher_methodical_missing * w.teacher_methodical_day_violation
            + self.position_violations * w.position_violation
            + self.hygiene_peak * w.hygiene_peak
            + self.hygiene_hardest_at_edge * w.hygiene_hardest_at_edge
    }

    /// Человекочитаемый список ненулевых счётчиков.
    pub fn lines(&self) -> Vec<(String, i64)> {
        let all = [
            ("Нерасставленные уроки", self.unplaced),
            ("Окна у классов", self.class_windows),
            ("Превышение макс. уроков в день", self.class_over_max),
            ("Меньше мин. уроков в день", self.class_under_min),
            ("Класс меняет корпус в течение дня", self.class_building_changes),
            ("Одиночные окна учителей", self.teacher_single_windows),
            ("Длинные окна учителей (2+ урока)", self.teacher_multi_windows),
            ("Дни учителя с одним уроком", self.teacher_one_lesson_days),
            ("Переходы учителей между корпусами", self.teacher_building_changes),
            ("Переход между корпусами без свободного урока", self.teacher_impossible_transitions),
            ("Уроки в «нежелательные» часы учителя", self.teacher_if_needed),
            ("Не выдержаны методические дни", self.teacher_methodical_missing),
            ("Нарушены требования к номеру урока", self.position_violations),
            ("Пик трудности не в заданные дни", self.hygiene_peak),
            ("Самый трудный урок дня — крайний", self.hygiene_hardest_at_edge),
        ];
        all.iter().filter(|(_, v)| *v != 0).map(|(k, v)| (k.to_string(), *v)).collect()
    }
}

/// Пробеги пустых ячеек между первой и последней занятой.
fn gap_runs(occupied: &[bool]) -> Vec<usize> {
    let first = occupied.iter().position(|x| *x);
    let last = occupied.iter().rposition(|x| *x);
    let (Some(f), Some(l)) = (first, last) else {
        return vec![];
    };
    let mut runs = vec![];
    let mut cur = 0;
    for &o in &occupied[f..=l] {
        if o {
            if cur > 0 {
                runs.push(cur);
            }
            cur = 0;
        } else {
            cur += 1;
        }
    }
    runs
}

/// `week[day][period]`
pub fn class_week(week: &[Vec<Option<ClassCell>>], class: &Class, s: &Settings) -> Stats {
    let mut st = Stats::default();
    let mut day_sums: Vec<(u8, u32)> = vec![];
    for (d, day) in week.iter().enumerate() {
        let occ: Vec<bool> = day.iter().map(|c| c.is_some()).collect();
        let n = occ.iter().filter(|x| **x).count() as i64;
        if n == 0 {
            continue;
        }
        let mut w: i64 = gap_runs(&occ).iter().map(|r| *r as i64).sum();
        if !s.classes_may_start_later {
            w += occ.iter().position(|x| *x).unwrap_or(0) as i64;
        }
        st.class_windows += w;
        if n > class.max_per_day as i64 {
            st.class_over_max += n - class.max_per_day as i64;
        }
        if n < class.min_per_day as i64 {
            st.class_under_min += class.min_per_day as i64 - n;
        }
        // корпуса
        let mut prev_b: Option<Id> = None;
        for c in day.iter().flatten() {
            if let (Some(p), Some(b)) = (prev_b, c.building) {
                if p != b {
                    st.class_building_changes += 1;
                }
            }
            if c.building.is_some() {
                prev_b = c.building;
            }
        }
        // требования к номеру урока
        let first = occ.iter().position(|x| *x).unwrap();
        let last = occ.iter().rposition(|x| *x).unwrap();
        for (p, c) in day.iter().enumerate() {
            let Some(c) = c else { continue };
            let ok = match c.position {
                PositionReq::Any => true,
                PositionReq::First => p == first || (p > first && same_run_from(day, first, p)),
                PositionReq::Last => p == last || (p < last && same_run_to(day, p, last)),
                PositionReq::Edge => p == first || p == last || same_run_from(day, first, p) || same_run_to(day, p, last),
            };
            if !ok {
                st.position_violations += 1;
            }
        }
        // самый трудный урок дня не должен быть первым или последним
        if n >= 3 {
            let max = day.iter().flatten().map(|c| c.difficulty).max().unwrap_or(0);
            let hardest: Vec<usize> = day.iter().enumerate().filter(|(_, c)| c.map(|c| c.difficulty) == Some(max)).map(|(p, _)| p).collect();
            if max > 0 && hardest.iter().all(|p| *p == first || *p == last) {
                st.hygiene_hardest_at_edge += 1;
            }
        }
        let sum: u32 = day.iter().flatten().map(|c| c.difficulty as u32).sum();
        day_sums.push((d as u8, sum));
    }
    let hp = &s.hygiene;
    if !hp.peak_days.is_empty() && day_sums.len() >= 3 {
        let max = day_sums.iter().map(|x| x.1).max().unwrap_or(0);
        if max > 0 && !day_sums.iter().any(|(d, v)| *v == max && hp.peak_days.contains(d)) {
            st.hygiene_peak += 1;
        }
        if !hp.light_days.is_empty() {
            let avg = day_sums.iter().map(|x| x.1).sum::<u32>() / day_sums.len() as u32;
            for (d, v) in &day_sums {
                if hp.light_days.contains(d) && *v > avg {
                    st.hygiene_peak += 1;
                }
            }
        }
    }
    st
}

/// Сдвоенный урок «первый» занимает 1-й и 2-й — второй тоже считается выполненным.
fn same_run_from(day: &[Option<ClassCell>], from: usize, to: usize) -> bool {
    let pos = day[to].map(|c| c.position);
    (from..=to).all(|p| day[p].is_some() && day[p].map(|c| c.position) == pos)
}
fn same_run_to(day: &[Option<ClassCell>], from: usize, to: usize) -> bool {
    same_run_from(day, from, to)
}

pub fn teacher_week(week: &[Vec<Option<TeacherCell>>], t: &Teacher, s: &Settings) -> Stats {
    let mut st = Stats::default();
    let mut free_working_days = 0i64;
    for (d, day) in week.iter().enumerate() {
        let occ: Vec<bool> = day.iter().map(|c| c.is_some()).collect();
        let n = occ.iter().filter(|x| **x).count();
        if n == 0 {
            let working = (0..day.len()).any(|p| t.availability.get(crate::model::Slot::new(d as u8, p as u8)) != crate::model::Avail::No);
            if working {
                free_working_days += 1;
            }
            continue;
        }
        if !t.ignore_windows {
            for r in gap_runs(&occ) {
                if r == 1 {
                    st.teacher_single_windows += 1;
                } else {
                    st.teacher_multi_windows += 1;
                }
            }
        }
        if n == 1 {
            st.teacher_one_lesson_days += 1;
        }
        st.teacher_if_needed += day.iter().flatten().filter(|c| c.if_needed).count() as i64;
        // переходы между корпусами
        let mut prev: Option<(usize, Id)> = None;
        for (p, c) in day.iter().enumerate() {
            let Some(c) = c else { continue };
            let Some(b) = c.building else { continue };
            if let Some((pp, pb)) = prev {
                if pb != b {
                    st.teacher_building_changes += 1;
                    // без свободного урока перейти нельзя — штраф как за жёсткое нарушение
                    if !s.break_enough_for_building_change && p == pp + 1 {
                        st.teacher_impossible_transitions += 1;
                    }
                }
            }
            prev = Some((p, b));
        }
    }
    if t.methodical_days as i64 > free_working_days {
        st.teacher_methodical_missing += t.methodical_days as i64 - free_working_days;
    }
    st
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gaps() {
        assert_eq!(gap_runs(&[false, true, false, true, false, false, true]), vec![1, 2]);
        assert!(gap_runs(&[false, false]).is_empty());
    }
}
