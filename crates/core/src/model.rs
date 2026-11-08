// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Модель данных SkyUrok. Всё, что нужно для расписания, хранится в одном
//! сериализуемом `Project` с явной схемой и версией.

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

pub type Id = u32;

pub const PROJECT_FORMAT_VERSION: u32 = 1;

/// Ячейка недельной сетки: день (0 = понедельник) и номер урока (0 = первый).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Slot {
    pub day: u8,
    pub period: u8,
}

impl Slot {
    pub fn new(day: u8, period: u8) -> Self {
        Self { day, period }
    }
}

pub const DAY_NAMES: [&str; 7] = ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс"];
pub const DAY_NAMES_FULL: [&str; 7] = ["Понедельник", "Вторник", "Среда", "Четверг", "Пятница", "Суббота", "Воскресенье"];

/// Доступность ячейки: можно / нежелательно / нельзя.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Avail {
    #[default]
    Yes,
    /// «Можно при необходимости» — генератор штрафует, ручной режим разрешает.
    IfNeeded,
    No,
}

/// Недельный график: по умолчанию всё доступно, хранятся только исключения.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Availability {
    #[serde(default)]
    pub blocked: Vec<Slot>,
    #[serde(default)]
    pub if_needed: Vec<Slot>,
    /// Целиком нерабочие дни.
    #[serde(default)]
    pub days_off: Vec<u8>,
}

impl Availability {
    pub fn get(&self, s: Slot) -> Avail {
        if self.days_off.contains(&s.day) || self.blocked.contains(&s) {
            Avail::No
        } else if self.if_needed.contains(&s) {
            Avail::IfNeeded
        } else {
            Avail::Yes
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub days_per_week: u8,
    pub periods_per_day: u8,
    /// Хватает ли перемены, чтобы перейти между корпусами. Если нет —
    /// учителю нужен свободный урок между уроками в разных корпусах.
    #[serde(default)]
    pub break_enough_for_building_change: bool,
    /// Классы могут приходить не к первому уроку (пропуск первого урока — не окно).
    #[serde(default)]
    pub classes_may_start_later: bool,
    /// Гигиенический профиль (дни пика трудности и т.п.) — настраиваемый,
    /// потому что нормы меняются (СанПиН 2.4.2.2821-10 → СанПиН 1.2.3685-21).
    #[serde(default)]
    pub hygiene: HygieneProfile,
    #[serde(default)]
    pub weights: Weights,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HygieneProfile {
    pub name: String,
    /// Дни, на которые должен приходиться пик суммарной трудности (0 = Пн).
    pub peak_days: Vec<u8>,
    /// Облегчённые дни (суммарная трудность должна быть ниже средней).
    pub light_days: Vec<u8>,
}

impl Default for HygieneProfile {
    fn default() -> Self {
        Self {
            name: "Пользовательский (проверьте по действующему СанПиН)".into(), peak_days: vec![1, 2], light_days: vec![]
        }
    }
}

/// Веса штрафов. Аналог диалога «Требования», но с понятными единицами.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Weights {
    pub unplaced: i64,
    pub class_window: i64,
    pub class_over_max: i64,
    pub class_under_min: i64,
    pub teacher_single_window: i64,
    pub teacher_multi_window: i64,
    pub teacher_one_lesson_day: i64,
    pub teacher_building_change: i64,
    pub teacher_if_needed_slot: i64,
    pub teacher_methodical_day_violation: i64,
    pub position_violation: i64,
    pub hygiene_peak: i64,
    pub hygiene_hardest_at_edge: i64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            unplaced: 1000,
            class_window: 40,
            class_over_max: 60,
            class_under_min: 15,
            teacher_single_window: 6,
            teacher_multi_window: 12,
            teacher_one_lesson_day: 5,
            teacher_building_change: 10,
            teacher_if_needed_slot: 4,
            teacher_methodical_day_violation: 8,
            position_violation: 25,
            hygiene_peak: 6,
            hygiene_hardest_at_edge: 2,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            days_per_week: 5,
            periods_per_day: 7,
            break_enough_for_building_change: false,
            classes_may_start_later: false,
            hygiene: HygieneProfile::default(),
            weights: Weights::default(),
        }
    }
}

/// Корпус / отдельное здание школы со своими звонками.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Building {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub address: String,
    /// Расписание звонков корпуса (у разных зданий оно бывает разным).
    #[serde(default)]
    pub bells: Vec<BellTime>,
    /// Время перехода (минуты) до других корпусов: (id корпуса, минуты).
    #[serde(default)]
    pub travel_minutes: Vec<(Id, u16)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BellTime {
    pub period: u8,
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Room {
    pub id: Id,
    pub name: String,
    pub building: Id,
    pub capacity: u32,
    /// Сколько групп одновременно вмещает (спортзал на 2 учителя = 2).
    #[serde(default = "one")]
    pub parallel_groups: u8,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub availability: Availability,
}

fn one() -> u8 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Subject {
    pub id: Id,
    pub name: String,
    pub short: String,
    /// Образовательная область — для подбора «родственных» заменяющих.
    #[serde(default)]
    pub area: String,
    /// Баллы трудности по ступеням: [1–4 кл., 5–9 кл., 10–11 кл.].
    #[serde(default)]
    pub difficulty: [u8; 3],
}

impl Subject {
    pub fn difficulty_for_grade(&self, grade: u8) -> u8 {
        match grade {
            0..=4 => self.difficulty[0],
            5..=9 => self.difficulty[1],
            _ => self.difficulty[2],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Teacher {
    pub id: Id,
    pub last_name: String,
    #[serde(default)]
    pub first_name: String,
    #[serde(default)]
    pub middle_name: String,
    /// Предметы, которые учитель может вести (для замен — ключевое поле).
    #[serde(default)]
    pub subjects: Vec<Id>,
    #[serde(default)]
    pub home_room: Option<Id>,
    #[serde(default)]
    pub availability: Availability,
    /// Не считать окна (завучи, директор).
    #[serde(default)]
    pub ignore_windows: bool,
    #[serde(default)]
    pub methodical_days: u8,
    /// Лимит замен в неделю (None — без лимита).
    #[serde(default)]
    pub max_substitutions_per_week: Option<u8>,
    /// Может ли вообще выходить на замены (совместители, декрет и т.п.).
    #[serde(default = "yes")]
    pub can_substitute: bool,
    #[serde(default)]
    pub phone: String,
    #[serde(default)]
    pub note: String,
}

fn yes() -> bool {
    true
}

impl Teacher {
    pub fn short_name(&self) -> String {
        let mut s = self.last_name.clone();
        if let Some(c) = self.first_name.chars().next() {
            s.push(' ');
            s.push(c);
            s.push('.');
            if let Some(m) = self.middle_name.chars().next() {
                s.push(m);
                s.push('.');
            }
        }
        s
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Class {
    pub id: Id,
    pub name: String,
    pub grade: u8,
    pub students: u32,
    #[serde(default)]
    pub shift: u8,
    /// Основной корпус класса. Класс не переходит между корпусами в течение дня.
    #[serde(default)]
    pub building: Option<Id>,
    pub max_per_day: u8,
    pub min_per_day: u8,
    #[serde(default)]
    pub availability: Availability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Pairing {
    /// Не больше двух уроков в день, рядом стоять не обязаны.
    #[default]
    Any,
    /// Только сдвоенные уроки (пара).
    Double,
    /// Не больше одного урока в день.
    NoDouble,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PositionReq {
    #[default]
    Any,
    First,
    Last,
    Edge,
}

/// Группа внутри нагрузки: свой учитель, предмет, кабинет(ы).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub teacher: Id,
    pub subject: Id,
    /// Допустимые кабинеты по порядку предпочтения. Пусто = любой подходящий.
    #[serde(default)]
    pub rooms: Vec<Id>,
    #[serde(default)]
    pub label: String,
    /// Доля учащихся (для деления на группы), по умолчанию — поровну.
    #[serde(default)]
    pub students: Option<u32>,
}

/// Нагрузка: один или несколько классов (поток), одна или несколько групп,
/// N часов в неделю. Группы занимаются одновременно.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Load {
    pub id: Id,
    pub classes: Vec<Id>,
    pub hours_per_week: u8,
    #[serde(default)]
    pub pairing: Pairing,
    #[serde(default)]
    pub position: PositionReq,
    pub groups: Vec<Group>,
    #[serde(default)]
    pub paid: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct School {
    pub name: String,
    pub settings: Settings,
    pub buildings: Vec<Building>,
    pub rooms: Vec<Room>,
    pub subjects: Vec<Subject>,
    pub teachers: Vec<Teacher>,
    pub classes: Vec<Class>,
    pub loads: Vec<Load>,
}

/// Один урок в сетке (сдвоенный урок = два `Lesson` подряд).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lesson {
    pub id: Id,
    pub load: Id,
    pub slot: Slot,
    /// Кабинет для каждой группы нагрузки (тот же порядок, что `Load::groups`).
    pub rooms: Vec<Id>,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Timetable {
    pub lessons: Vec<Lesson>,
}

/// Отсутствие учителя: диапазон дат, опционально только часть уроков.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Absence {
    pub id: Id,
    pub teacher: Id,
    pub from: NaiveDate,
    pub to: NaiveDate,
    #[serde(default)]
    pub reason: String,
    /// Если задано — отсутствует только на этих уроках (номера с 0).
    #[serde(default)]
    pub periods: Option<Vec<u8>>,
}

impl Absence {
    pub fn covers(&self, date: NaiveDate, period: u8) -> bool {
        date >= self.from && date <= self.to && self.periods.as_ref().is_none_or(|p| p.contains(&period))
    }
}

/// Отсутствие класса (экскурсия, ВПР, карантин): уроки не нужны, учителя свободны.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassAbsence {
    pub id: Id,
    pub class: Id,
    pub from: NaiveDate,
    pub to: NaiveDate,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub periods: Option<Vec<u8>>,
}

impl ClassAbsence {
    pub fn covers(&self, date: NaiveDate, period: u8) -> bool {
        date >= self.from && date <= self.to && self.periods.as_ref().is_none_or(|p| p.contains(&period))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ReplacementKind {
    /// Урок ведёт другой учитель (возможно, другой предмет).
    Substitute { teacher: Id, subject: Id },
    /// Класс(группа) присоединяется к другому уроку в том же слоте.
    Merge { with_lesson: Id, group: usize },
    /// Урок снят.
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Replacement {
    pub id: Id,
    pub date: NaiveDate,
    pub lesson: Id,
    /// Индекс группы в нагрузке (для деления класса на группы).
    pub group: usize,
    pub original_teacher: Id,
    pub kind: ReplacementKind,
    /// Новый кабинет (None — прежний).
    #[serde(default)]
    pub room: Option<Id>,
    #[serde(default)]
    pub note: String,
    /// Подтверждена ли замена человеком (автоподбор создаёт черновики).
    #[serde(default)]
    pub confirmed: bool,
}

/// Весь проект в одном файле.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub format_version: u32,
    pub school: School,
    #[serde(default)]
    pub timetable: Timetable,
    #[serde(default)]
    pub absences: Vec<Absence>,
    #[serde(default)]
    pub replacements: Vec<Replacement>,
    #[serde(default)]
    pub class_absences: Vec<ClassAbsence>,
}

impl Project {
    pub fn new(school: School) -> Self {
        Self { format_version: PROJECT_FORMAT_VERSION, school, timetable: Timetable::default(), absences: vec![], replacements: vec![], class_absences: vec![] }
    }

    pub fn load_json(text: &str) -> Result<Self, String> {
        let p: Project = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if p.format_version > PROJECT_FORMAT_VERSION {
            return Err(format!("Файл создан более новой версией (формат {}), эта версия читает до {}", p.format_version, PROJECT_FORMAT_VERSION));
        }
        Ok(p)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("serialize")
    }
}

/// Быстрый доступ к сущностям по id.
pub struct Index<'a> {
    pub school: &'a School,
}

impl<'a> Index<'a> {
    pub fn new(school: &'a School) -> Self {
        Self { school }
    }
    pub fn teacher(&self, id: Id) -> Option<&'a Teacher> {
        self.school.teachers.iter().find(|t| t.id == id)
    }
    pub fn class(&self, id: Id) -> Option<&'a Class> {
        self.school.classes.iter().find(|c| c.id == id)
    }
    pub fn room(&self, id: Id) -> Option<&'a Room> {
        self.school.rooms.iter().find(|r| r.id == id)
    }
    pub fn subject(&self, id: Id) -> Option<&'a Subject> {
        self.school.subjects.iter().find(|s| s.id == id)
    }
    pub fn load(&self, id: Id) -> Option<&'a Load> {
        self.school.loads.iter().find(|l| l.id == id)
    }
    pub fn building(&self, id: Id) -> Option<&'a Building> {
        self.school.buildings.iter().find(|b| b.id == id)
    }
    pub fn teacher_name(&self, id: Id) -> String {
        self.teacher(id).map(|t| t.short_name()).unwrap_or_else(|| format!("#{id}"))
    }
    pub fn class_name(&self, id: Id) -> String {
        self.class(id).map(|c| c.name.clone()).unwrap_or_else(|| format!("#{id}"))
    }
    pub fn room_name(&self, id: Id) -> String {
        match self.room(id) {
            Some(r) if self.school.buildings.len() > 1 => {
                let b = self.building(r.building).map(|b| b.name.as_str()).unwrap_or("?");
                format!("{} ({})", r.name, b)
            }
            Some(r) => r.name.clone(),
            None => format!("#{id}"),
        }
    }
    pub fn subject_name(&self, id: Id) -> String {
        self.subject(id).map(|s| s.name.clone()).unwrap_or_else(|| format!("#{id}"))
    }
    pub fn classes_label(&self, ids: &[Id]) -> String {
        ids.iter().map(|c| self.class_name(*c)).collect::<Vec<_>>().join("+")
    }
    pub fn students_in(&self, load: &Load, group: usize) -> u32 {
        let g = &load.groups[group];
        if let Some(n) = g.students {
            return n;
        }
        let total: u32 = load.classes.iter().filter_map(|c| self.class(*c)).map(|c| c.students).sum();
        total / load.groups.len().max(1) as u32
    }
}

/// День недели сетки по календарной дате (None — выходной для этой сетки).
pub fn day_of(date: NaiveDate, days_per_week: u8) -> Option<u8> {
    let d = date.weekday().num_days_from_monday() as u8;
    (d < days_per_week).then_some(d)
}
