// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Генератор расписания: жадная расстановка «самых трудных первыми» +
//! имитация отжига с выталкиванием конфликтующих уроков. Жёсткие требования
//! не нарушаются никогда, мягкие — минимизируются по весам `Weights`.
//! Детерминирован при одинаковом `seed` (воспроизводимые варианты).

use crate::grid::empty_week;
use crate::model::*;
use crate::score::{class_week, teacher_week, ClassCell, TeacherCell};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Прогресс генерации для интерфейса (читается из другого потока).
#[derive(Debug, Default)]
pub struct SolveProgress {
    pub iterations: AtomicU64,
    pub best_score: AtomicI64,
    pub unplaced: AtomicU64,
    /// 0 — подготовка, 1 — жадная расстановка, 2 — улучшение.
    pub phase: AtomicU64,
}

#[derive(Debug, Clone)]
pub struct SolveOptions {
    pub seed: u64,
    pub max_iterations: u64,
    pub time_limit: Duration,
    /// Закреплённые уроки из этого расписания сохраняются как есть.
    pub keep_pinned_from: Option<Timetable>,
    /// Начать с этого расписания (режим «Улучшить»), иначе — с нуля.
    pub start_from: Option<Timetable>,
    pub stop: Option<Arc<AtomicBool>>,
    pub progress: Option<Arc<SolveProgress>>,
}

impl Default for SolveOptions {
    fn default() -> Self {
        Self { seed: 1, max_iterations: 400_000, time_limit: Duration::from_secs(20), keep_pinned_from: None, start_from: None, stop: None, progress: None }
    }
}

impl SolveOptions {
    fn stopped(&self) -> bool {
        self.stop.as_ref().is_some_and(|s| s.load(Ordering::Relaxed))
    }
}

#[derive(Debug, Clone)]
pub struct SolveResult {
    pub timetable: Timetable,
    pub score: i64,
    pub unplaced: usize,
    pub iterations: u64,
    pub elapsed: Duration,
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

struct LoadInfo {
    load_id: Id,
    classes: Vec<usize>,
    teachers: Vec<usize>,
    room_cands: Vec<Vec<usize>>,
    pairing: Pairing,
    /// трудность по каждому классу нагрузки
    difficulty: Vec<u8>,
    position: PositionReq,
}

#[derive(Clone)]
struct Unit {
    load: usize,
    len: u8,
    pinned: bool,
}

type Place = Option<(Slot, Vec<usize>)>;

struct Solver<'a> {
    school: &'a School,
    days: u8,
    periods: u8,
    loads: Vec<LoadInfo>,
    units: Vec<Unit>,
    load_units: Vec<Vec<usize>>,
    place: Vec<Place>,
    class_grid: Vec<Vec<Option<usize>>>,
    teacher_grid: Vec<Vec<Option<usize>>>,
    room_cnt: Vec<Vec<u8>>,
    class_building: Vec<Option<Id>>,
}

impl<'a> Solver<'a> {
    fn cell(&self, s: Slot) -> usize {
        s.day as usize * self.periods as usize + s.period as usize
    }

    fn new(school: &'a School, opts: &SolveOptions) -> Self {
        let ix = Index::new(school);
        let s = &school.settings;
        let cidx = |id: Id| school.classes.iter().position(|c| c.id == id);
        let tidx = |id: Id| school.teachers.iter().position(|t| t.id == id);
        let ridx = |id: Id| school.rooms.iter().position(|r| r.id == id);
        let mut loads = vec![];
        for l in &school.loads {
            let classes: Vec<usize> = l.classes.iter().filter_map(|c| cidx(*c)).collect();
            let building = classes.iter().find_map(|c| school.classes[*c].building);
            let teachers: Vec<usize> = l.groups.iter().map(|g| tidx(g.teacher).unwrap_or(usize::MAX)).collect();
            let mut room_cands = vec![];
            for (gi, g) in l.groups.iter().enumerate() {
                let need = ix.students_in(l, gi);
                let mut c: Vec<usize> = g.rooms.iter().filter_map(|r| ridx(*r)).collect();
                if c.is_empty() {
                    if let Some(home) = ix.teacher(g.teacher).and_then(|t| t.home_room).and_then(ridx) {
                        c.push(home);
                    }
                    let mut rest: Vec<usize> = (0..school.rooms.len())
                        .filter(|r| {
                            let room = &school.rooms[*r];
                            room.capacity >= need && room.tags.is_empty() && building.is_none_or(|b| room.building == b) && !c.contains(r)
                        })
                        .collect();
                    rest.sort_by_key(|r| school.rooms[*r].capacity);
                    c.extend(rest);
                }
                room_cands.push(c);
            }
            let difficulty = l
                .classes
                .iter()
                .map(|c| {
                    let grade = ix.class(*c).map(|c| c.grade).unwrap_or(5);
                    l.groups.iter().filter_map(|g| ix.subject(g.subject)).map(|s| s.difficulty_for_grade(grade)).max().unwrap_or(0)
                })
                .collect();
            loads.push(LoadInfo { load_id: l.id, classes, teachers, room_cands, pairing: l.pairing, difficulty, position: l.position });
        }
        let cells = s.days_per_week as usize * s.periods_per_day as usize;
        let mut me = Solver {
            school,
            days: s.days_per_week,
            periods: s.periods_per_day,
            units: vec![],
            load_units: vec![vec![]; loads.len()],
            place: vec![],
            class_grid: vec![vec![None; cells]; school.classes.len()],
            teacher_grid: vec![vec![None; cells]; school.teachers.len()],
            room_cnt: vec![vec![0; cells]; school.rooms.len()],
            class_building: school.classes.iter().map(|c| c.building).collect(),
            loads,
        };
        // закреплённые уроки
        let mut pinned_hours = vec![0u8; school.loads.len()];
        if let Some(tt) = &opts.keep_pinned_from {
            for les in tt.lessons.iter().filter(|l| l.pinned) {
                let Some(li) = school.loads.iter().position(|l| l.id == les.load) else {
                    continue;
                };
                let rooms: Vec<usize> = les.rooms.iter().filter_map(|r| ridx(*r)).collect();
                if rooms.len() != me.loads[li].teachers.len() {
                    continue;
                }
                let u = me.units.len();
                me.units.push(Unit { load: li, len: 1, pinned: true });
                me.load_units[li].push(u);
                me.place.push(None);
                me.apply(u, les.slot, rooms);
                pinned_hours[li] += 1;
            }
        }
        let first_free_unit = me.units.len();
        for (li, l) in school.loads.iter().enumerate() {
            let mut left = l.hours_per_week.saturating_sub(pinned_hours[li]);
            while left > 0 {
                let len = if l.pairing == Pairing::Double && left >= 2 { 2 } else { 1 };
                let u = me.units.len();
                me.units.push(Unit { load: li, len, pinned: false });
                me.load_units[li].push(u);
                me.place.push(None);
                left -= len;
            }
        }
        if let Some(tt) = &opts.start_from {
            me.seed_from(tt, first_free_unit);
        }
        me
    }

    /// Поставить незакреплённые юниты туда, где они стоят в `tt` (если это допустимо).
    fn seed_from(&mut self, tt: &Timetable, first_free_unit: usize) {
        for li in 0..self.loads.len() {
            let id = self.loads[li].load_id;
            let mut slots: Vec<Slot> = tt.lessons.iter().filter(|l| l.load == id && !l.pinned).map(|l| l.slot).collect();
            slots.sort();
            let units: Vec<usize> = self.load_units[li].iter().copied().filter(|u| *u >= first_free_unit).collect();
            for u in units {
                let len = self.units[u].len;
                let pos = slots.iter().position(|s| len == 1 || slots.contains(&Slot::new(s.day, s.period + 1)));
                let Some(pos) = pos else { continue };
                let s = slots[pos];
                if let Some(rooms) = self.feasible(u, s) {
                    self.apply(u, s, rooms);
                    slots.retain(|x| !(x.day == s.day && x.period >= s.period && x.period < s.period + len));
                }
            }
        }
    }

    fn day_count(&self, li: usize, day: u8, except: usize) -> (u8, u8) {
        let mut lessons = 0;
        let mut units = 0;
        for &u in &self.load_units[li] {
            if u == except {
                continue;
            }
            if let Some((s, _)) = &self.place[u] {
                if s.day == day {
                    lessons += self.units[u].len;
                    units += 1;
                }
            }
        }
        (lessons, units)
    }

    /// Можно ли поставить юнит в слот; возвращает выбранные кабинеты.
    fn feasible(&self, u: usize, slot: Slot) -> Option<Vec<usize>> {
        let unit = &self.units[u];
        let l = &self.loads[unit.load];
        if slot.period + unit.len > self.periods || slot.day >= self.days {
            return None;
        }
        let (lessons, units) = self.day_count(unit.load, slot.day, u);
        let ok_day = match l.pairing {
            Pairing::Any => lessons + unit.len <= 2,
            Pairing::NoDouble => lessons == 0,
            Pairing::Double => units == 0,
        };
        if !ok_day {
            return None;
        }
        for t in 0..unit.len {
            let s = Slot::new(slot.day, slot.period + t);
            let c = self.cell(s);
            for &ci in &l.classes {
                if self.school.classes[ci].availability.get(s) == Avail::No {
                    return None;
                }
                if self.class_grid[ci][c].is_some_and(|x| x != u) {
                    return None;
                }
            }
            for &ti in &l.teachers {
                if ti == usize::MAX {
                    return None;
                }
                if self.school.teachers[ti].availability.get(s) == Avail::No {
                    return None;
                }
                if self.teacher_grid[ti][c].is_some_and(|x| x != u) {
                    return None;
                }
            }
        }
        // корпус класса в этот день
        let mut day_building: Option<Id> = l.classes.iter().find_map(|c| self.class_building[*c]);
        if day_building.is_none() {
            'outer: for &ci in &l.classes {
                for p in 0..self.periods {
                    let c = self.cell(Slot::new(slot.day, p));
                    if let Some(o) = self.class_grid[ci][c] {
                        if o != u {
                            if let Some((_, rooms)) = &self.place[o] {
                                if let Some(r) = rooms.first() {
                                    day_building = Some(self.school.rooms[*r].building);
                                    break 'outer;
                                }
                            }
                        }
                    }
                }
            }
        }
        let current: Option<&Vec<usize>> = self.place[u].as_ref().map(|p| &p.1);
        let mut chosen: Vec<usize> = vec![];
        for (gi, cands) in l.room_cands.iter().enumerate() {
            let mut found = None;
            for &r in cands {
                let room = &self.school.rooms[r];
                if day_building.is_some_and(|b| b != room.building) {
                    continue;
                }
                let used_here = chosen.iter().filter(|x| **x == r).count() as u8;
                let mut ok = true;
                for t in 0..unit.len {
                    let s = Slot::new(slot.day, slot.period + t);
                    if room.availability.get(s) == Avail::No {
                        ok = false;
                        break;
                    }
                    let mut cnt = self.room_cnt[r][self.cell(s)];
                    // если юнит уже стоит здесь — не считать самого себя
                    if let (Some(cur), Some((ps, _))) = (current, &self.place[u]) {
                        if cur.get(gi) == Some(&r) && ps.day == s.day && s.period >= ps.period && s.period < ps.period + unit.len {
                            cnt = cnt.saturating_sub(1);
                        }
                    }
                    if cnt + used_here >= room.parallel_groups {
                        ok = false;
                        break;
                    }
                }
                if ok {
                    found = Some(r);
                    break;
                }
            }
            chosen.push(found?);
            if day_building.is_none() {
                day_building = Some(self.school.rooms[*chosen.last().unwrap()].building);
            }
        }
        Some(chosen)
    }

    fn apply(&mut self, u: usize, slot: Slot, rooms: Vec<usize>) {
        let len = self.units[u].len;
        let li = self.units[u].load;
        for t in 0..len {
            let c = self.cell(Slot::new(slot.day, slot.period + t));
            for &ci in &self.loads[li].classes {
                self.class_grid[ci][c] = Some(u);
            }
            for &ti in &self.loads[li].teachers {
                if ti != usize::MAX {
                    self.teacher_grid[ti][c] = Some(u);
                }
            }
            for &r in &rooms {
                self.room_cnt[r][c] += 1;
            }
        }
        self.place[u] = Some((slot, rooms));
    }

    fn remove(&mut self, u: usize) -> Place {
        let p = self.place[u].take();
        if let Some((slot, rooms)) = &p {
            let len = self.units[u].len;
            let li = self.units[u].load;
            for t in 0..len {
                let c = self.cell(Slot::new(slot.day, slot.period + t));
                for &ci in &self.loads[li].classes {
                    if self.class_grid[ci][c] == Some(u) {
                        self.class_grid[ci][c] = None;
                    }
                }
                for &ti in &self.loads[li].teachers {
                    if ti != usize::MAX && self.teacher_grid[ti][c] == Some(u) {
                        self.teacher_grid[ti][c] = None;
                    }
                }
                for &r in rooms {
                    self.room_cnt[r][c] -= 1;
                }
            }
        }
        p
    }

    fn class_pen(&self, ci: usize) -> i64 {
        let s = &self.school.settings;
        let mut w: Vec<Vec<Option<ClassCell>>> = empty_week(s);
        for d in 0..self.days {
            for p in 0..self.periods {
                if let Some(u) = self.class_grid[ci][self.cell(Slot::new(d, p))] {
                    let li = self.units[u].load;
                    let l = &self.loads[li];
                    let k = l.classes.iter().position(|c| *c == ci).unwrap_or(0);
                    let building = self.place[u].as_ref().and_then(|(_, r)| r.first()).map(|r| self.school.rooms[*r].building);
                    w[d as usize][p as usize] = Some(ClassCell { difficulty: l.difficulty[k], position: l.position, building });
                }
            }
        }
        class_week(&w, &self.school.classes[ci], s).total(&s.weights)
    }

    fn teacher_pen(&self, ti: usize) -> i64 {
        let s = &self.school.settings;
        let t = &self.school.teachers[ti];
        let mut w: Vec<Vec<Option<TeacherCell>>> = empty_week(s);
        for d in 0..self.days {
            for p in 0..self.periods {
                let slot = Slot::new(d, p);
                if let Some(u) = self.teacher_grid[ti][self.cell(slot)] {
                    let l = &self.loads[self.units[u].load];
                    let gi = l.teachers.iter().position(|x| *x == ti).unwrap_or(0);
                    let building = self.place[u].as_ref().and_then(|(_, r)| r.get(gi)).map(|r| self.school.rooms[*r].building);
                    w[d as usize][p as usize] = Some(TeacherCell { building, if_needed: t.availability.get(slot) == Avail::IfNeeded });
                }
            }
        }
        teacher_week(&w, t, s).total(&s.weights)
    }

    fn entities(&self, units: &[usize]) -> (Vec<usize>, Vec<usize>) {
        let mut cs = HashSet::new();
        let mut ts = HashSet::new();
        for &u in units {
            let l = &self.loads[self.units[u].load];
            cs.extend(l.classes.iter().copied());
            ts.extend(l.teachers.iter().copied().filter(|t| *t != usize::MAX));
        }
        (cs.into_iter().collect(), ts.into_iter().collect())
    }

    fn pen_of(&self, e: &(Vec<usize>, Vec<usize>)) -> i64 {
        e.0.iter().map(|c| self.class_pen(*c)).sum::<i64>() + e.1.iter().map(|t| self.teacher_pen(*t)).sum::<i64>()
    }

    fn unplaced(&self) -> usize {
        self.place.iter().filter(|p| p.is_none()).count()
    }

    fn total(&self) -> i64 {
        let s = &self.school.settings;
        (0..self.school.classes.len()).map(|c| self.class_pen(c)).sum::<i64>()
            + (0..self.school.teachers.len()).map(|t| self.teacher_pen(t)).sum::<i64>()
            + self.unplaced() as i64 * s.weights.unplaced
    }

    fn all_slots(&self) -> Vec<Slot> {
        (0..self.days).flat_map(|d| (0..self.periods).map(move |p| Slot::new(d, p))).collect()
    }

    /// Юниты, мешающие поставить `u` в `slot` (по классам и учителям).
    fn blockers(&self, u: usize, slot: Slot) -> Option<Vec<usize>> {
        let unit = &self.units[u];
        if slot.period + unit.len > self.periods {
            return None;
        }
        let l = &self.loads[unit.load];
        let mut v = vec![];
        for t in 0..unit.len {
            let c = self.cell(Slot::new(slot.day, slot.period + t));
            for &ci in &l.classes {
                if let Some(o) = self.class_grid[ci][c] {
                    if o != u {
                        v.push(o);
                    }
                }
            }
            for &ti in &l.teachers {
                if ti == usize::MAX {
                    return None;
                }
                if let Some(o) = self.teacher_grid[ti][c] {
                    if o != u {
                        v.push(o);
                    }
                }
            }
        }
        v.sort();
        v.dedup();
        if v.iter().any(|o| self.units[*o].pinned) {
            return None;
        }
        Some(v)
    }

    fn greedy(&mut self) {
        let slots = self.all_slots();
        let mut order: Vec<usize> = (0..self.units.len()).filter(|u| self.place[*u].is_none()).collect();
        let freedom: Vec<usize> = (0..self.units.len()).map(|u| slots.iter().filter(|s| self.feasible(u, **s).is_some()).count()).collect();
        order.sort_by_key(|u| (freedom[*u], std::cmp::Reverse(self.units[*u].len), *u));
        for u in order {
            let ents = self.entities(&[u]);
            let before = self.pen_of(&ents);
            let mut best: Option<(i64, Slot, Vec<usize>)> = None;
            for &s in &slots {
                if let Some(rooms) = self.feasible(u, s) {
                    self.apply(u, s, rooms.clone());
                    let d = self.pen_of(&ents) - before;
                    self.remove(u);
                    if best.as_ref().is_none_or(|b| d < b.0) {
                        best = Some((d, s, rooms));
                    }
                }
            }
            if let Some((_, s, r)) = best {
                self.apply(u, s, r);
            }
        }
    }

    fn anneal(&mut self, opts: &SolveOptions) -> u64 {
        let w_unplaced = self.school.settings.weights.unplaced;
        let start = Instant::now();
        let mut rng = Rng(opts.seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
        let movable: Vec<usize> = (0..self.units.len()).filter(|u| !self.units[*u].pinned).collect();
        if movable.is_empty() {
            return 0;
        }
        let slots = self.all_slots();
        let mut cur = self.total();
        let mut best = cur;
        let mut best_place = self.place.clone();
        let t0 = 30.0f64;
        let t1 = 0.2f64;
        let mut it = 0u64;
        // Охлаждение: по доле итераций, а если их число не ограничено (так работает
        // программа — «считать N секунд»), то по доле прошедшего времени. Без этого
        // температура не падала и результат в программе был заметно хуже, чем в консоли.
        let by_time = opts.max_iterations > 100_000_000;
        let limit_s = opts.time_limit.as_secs_f64().max(0.001);
        let mut time_frac = 0.0f64;
        while it < opts.max_iterations {
            if it.is_multiple_of(512) {
                if start.elapsed() > opts.time_limit || opts.stopped() {
                    break;
                }
                time_frac = (start.elapsed().as_secs_f64() / limit_s).min(1.0);
                if let Some(p) = &opts.progress {
                    p.iterations.store(it, Ordering::Relaxed);
                    p.best_score.store(best, Ordering::Relaxed);
                    p.unplaced.store(best_place.iter().filter(|x| x.is_none()).count() as u64, Ordering::Relaxed);
                }
            }
            if best == 0 {
                break;
            }
            let frac = if by_time { time_frac } else { it as f64 / opts.max_iterations as f64 };
            let temp = t0 * (t1 / t0).powf(frac);
            it += 1;
            // выбор юнита: с вероятностью 1/2 — нерасставленный, если есть
            let unplaced: Vec<usize> = if rng.below(2) == 0 { movable.iter().copied().filter(|u| self.place[*u].is_none()).collect() } else { vec![] };
            let u = if !unplaced.is_empty() { unplaced[rng.below(unplaced.len())] } else { movable[rng.below(movable.len())] };
            let slot = slots[rng.below(slots.len())];
            let Some(block) = self.blockers(u, slot) else {
                continue;
            };
            if block.len() > 2 {
                continue;
            }
            let mut involved = block.clone();
            involved.push(u);
            let ents = self.entities(&involved);
            let before = self.pen_of(&ents);
            let unpl_before = involved.iter().filter(|x| self.place[**x].is_none()).count() as i64;
            let saved: Vec<(usize, Place)> = involved.iter().map(|x| (*x, self.place[*x].clone())).collect();
            for &b in &block {
                self.remove(b);
            }
            let old_u = self.remove(u);
            let rooms = self.feasible(u, slot);
            let Some(rooms) = rooms else {
                // откат
                if let Some((s, r)) = old_u {
                    self.apply(u, s, r);
                }
                for (x, p) in &saved {
                    if *x != u {
                        if let Some((s, r)) = p.clone() {
                            self.apply(*x, s, r);
                        }
                    }
                }
                continue;
            };
            self.apply(u, slot, rooms);
            // попытаться сразу вернуть вытолкнутые в старое место u
            if let Some((os, _)) = &old_u {
                for &b in &block {
                    if let Some(r) = self.feasible(b, *os) {
                        self.apply(b, *os, r);
                    }
                }
            }
            let after = self.pen_of(&ents);
            let unpl_after = involved.iter().filter(|x| self.place[**x].is_none()).count() as i64;
            let delta = after - before + (unpl_after - unpl_before) * w_unplaced;
            if delta <= 0 || rng.unit() < (-(delta as f64) / temp).exp() {
                cur += delta;
                if cur < best {
                    best = cur;
                    best_place = self.place.clone();
                }
            } else {
                for &x in &involved {
                    self.remove(x);
                }
                for (x, p) in saved {
                    if let Some((s, r)) = p {
                        self.apply(x, s, r);
                    }
                }
            }
        }
        // восстановить лучшее
        for u in 0..self.units.len() {
            self.remove(u);
        }
        for (u, p) in best_place.into_iter().enumerate() {
            if let Some((s, r)) = p {
                self.apply(u, s, r);
            }
        }
        it
    }

    fn timetable(&self) -> Timetable {
        let mut lessons = vec![];
        let mut id = 1;
        for (u, p) in self.place.iter().enumerate() {
            let Some((slot, rooms)) = p else { continue };
            let unit = &self.units[u];
            for t in 0..unit.len {
                lessons.push(Lesson {
                    id,
                    load: self.loads[unit.load].load_id,
                    slot: Slot::new(slot.day, slot.period + t),
                    rooms: rooms.iter().map(|r| self.school.rooms[*r].id).collect(),
                    pinned: unit.pinned,
                });
                id += 1;
            }
        }
        lessons.sort_by_key(|l| (l.slot, l.load));
        Timetable { lessons }
    }
}

pub fn solve(school: &School, opts: &SolveOptions) -> SolveResult {
    let start = Instant::now();
    if let Some(p) = &opts.progress {
        p.phase.store(0, Ordering::Relaxed);
    }
    let mut s = Solver::new(school, opts);
    if let Some(p) = &opts.progress {
        p.phase.store(1, Ordering::Relaxed);
    }
    s.greedy();
    if let Some(p) = &opts.progress {
        p.phase.store(2, Ordering::Relaxed);
        p.best_score.store(s.total(), Ordering::Relaxed);
        p.unplaced.store(s.unplaced() as u64, Ordering::Relaxed);
    }
    let iterations = s.anneal(opts);
    let score = s.total();
    SolveResult { timetable: s.timetable(), score, unplaced: s.unplaced(), iterations, elapsed: start.elapsed() }
}

/// Сколько вариантов считать одновременно: по ядру на вариант, но не больше 4.
pub fn default_threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, 4)
}

/// Считает `threads` вариантов параллельно (разные зёрна) и возвращает лучший:
/// сначала по нерасставленным часам, затем по оценке. Прогресс показывает первый вариант.
/// При одинаковых зерне и числе потоков результат одинаковый.
pub fn solve_parallel(school: &School, opts: &SolveOptions, threads: usize) -> SolveResult {
    let n = threads.max(1);
    if n == 1 {
        return solve(school, opts);
    }
    std::thread::scope(|sc| {
        let handles: Vec<_> = (0..n)
            .map(|i| {
                let mut o = opts.clone();
                o.seed = opts.seed.wrapping_add(i as u64 * 7919);
                if i > 0 {
                    o.progress = None;
                }
                sc.spawn(move || solve(school, &o))
            })
            .collect();
        let results: Vec<SolveResult> = handles.into_iter().filter_map(|h| h.join().ok()).collect();
        let iterations = results.iter().map(|r| r.iterations).sum();
        let mut best = results.into_iter().min_by_key(|r| (r.unplaced, r.score)).expect("хотя бы один вариант");
        best.iterations = iterations;
        best
    })
}
