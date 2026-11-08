// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Чтение файлов .rtt для переноса данных из других программ расписания.
//!
//! Формат восстановлен анализом файлов данных (не программы): последовательность
//! секций `[Имя]` + двоичные записи little-endian, строки — Delphi ShortString
//! (байт длины + cp1251) в полях фиксированной ширины, хвост поля — мусор.
//! Уверенно читаются: Room, Subject, Area, Class (основные поля).
//! Teacher / Load / Schedule / Plan / PeriodTimes — следующий этап (см. docs/rtt-format.md).

use crate::model::*;
use encoding_rs::WINDOWS_1251;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Section {
    pub name: String,
    pub offset: usize,
    #[serde(skip)]
    pub data: Vec<u8>,
    pub size: usize,
}

pub fn sections(bytes: &[u8]) -> Vec<Section> {
    let mut marks = vec![];
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            let mut j = i + 1;
            while j < bytes.len() && j - i < 40 && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b']' && j > i + 1 && bytes[i + 1].is_ascii_alphabetic() {
                marks.push((i, j + 1, String::from_utf8_lossy(&bytes[i + 1..j]).to_string()));
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    let mut out = vec![];
    for (k, (start, body, name)) in marks.iter().enumerate() {
        let end = marks.get(k + 1).map(|m| m.0).unwrap_or(bytes.len());
        out.push(Section { name: name.clone(), offset: *start, data: bytes[*body..end].to_vec(), size: end - body });
    }
    out
}

fn u32le(b: &[u8], o: usize) -> Option<i32> {
    b.get(o..o + 4).map(|x| i32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

/// ShortString в поле шириной `width` байт (включая байт длины).
fn sstr(b: &[u8], o: usize, width: usize) -> Option<String> {
    let len = *b.get(o)? as usize;
    if len >= width {
        return None;
    }
    let raw = b.get(o + 1..o + 1 + len)?;
    Some(WINDOWS_1251.decode(raw).0.trim().to_string())
}

#[derive(Debug, Clone, Serialize)]
pub struct RttRoom {
    pub id: i32,
    pub name: String,
    pub capacity: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RttSubject {
    pub id: i32,
    pub name: String,
    pub short: String,
    /// Баллы трудности по годам обучения 1..11.
    pub difficulty: [i32; 11],
    pub area: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RttArea {
    pub id: i32,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RttClass {
    pub id: i32,
    pub name: String,
    pub shift: i32,
    pub students: i32,
    pub min_per_day: i32,
    pub max_per_day: i32,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RttLists {
    pub sections: Vec<Section>,
    pub rooms: Vec<RttRoom>,
    pub subjects: Vec<RttSubject>,
    pub areas: Vec<RttArea>,
    pub classes: Vec<RttClass>,
    pub warnings: Vec<String>,
}

fn fixed<T>(data: &[u8], stride: usize, f: impl Fn(&[u8]) -> Option<T>) -> Result<Vec<T>, String> {
    let n = u32le(data, 0).ok_or("пустая секция")?;
    if n < 0 || 4 + n as usize * stride > data.len() {
        return Err(format!("неожиданный размер: {} записей по {stride} байт в {} байтах", n, data.len()));
    }
    (0..n as usize).map(|i| f(&data[4 + i * stride..4 + (i + 1) * stride]).ok_or_else(|| format!("запись {i} не читается"))).collect()
}

/// Запись класса: id(4) имя(12) смена,учеников,мин,макс (4×4) график(56) + хвост
/// переменной длины. Начало следующей записи ищется по сигнатуре.
fn classes(data: &[u8]) -> Result<Vec<RttClass>, String> {
    let n = u32le(data, 0).ok_or("пустая секция")? as usize;
    let plausible = |q: usize| -> bool {
        let (Some(id), Some(&len)) = (u32le(data, q), data.get(q + 4)) else {
            return false;
        };
        id > 0 && id < 1_000_000 && (1..=11).contains(&len) && data.get(q + 5..q + 5 + len as usize).is_some_and(|s| s.iter().all(|c| *c >= 0x20))
    };
    let mut out = vec![];
    let mut p = 4;
    for k in 0..n {
        if !plausible(p) {
            return Err(format!("класс {k}: не найдено начало записи"));
        }
        out.push(RttClass {
            id: u32le(data, p).unwrap(),
            name: sstr(data, p + 4, 12).unwrap_or_default(),
            shift: u32le(data, p + 16).unwrap_or(0),
            students: u32le(data, p + 20).unwrap_or(0),
            min_per_day: u32le(data, p + 24).unwrap_or(0),
            max_per_day: u32le(data, p + 28).unwrap_or(0),
        });
        if k + 1 < n {
            p = (p + 88..p + 140).find(|q| plausible(*q)).ok_or(format!("класс {k}: не найдена следующая запись"))?;
        }
    }
    Ok(out)
}

pub fn read_lists(bytes: &[u8]) -> Result<RttLists, String> {
    let secs = sections(bytes);
    if secs.first().map(|s| s.name.as_str()) != Some("General") {
        return Err("это не файл .rtt (нет секции [General] в начале)".into());
    }
    let get = |n: &str| secs.iter().find(|s| s.name == n).map(|s| s.data.as_slice());
    let mut l = RttLists { sections: secs.clone(), ..Default::default() };
    if let Some(d) = get("Room") {
        match fixed(d, 36, |r| Some(RttRoom { id: u32le(r, 0)?, name: sstr(r, 4, 16)?, capacity: u32le(r, 20)? })) {
            Ok(v) => l.rooms = v,
            Err(e) => l.warnings.push(format!("Кабинеты: {e}")),
        }
    }
    if let Some(d) = get("Subject") {
        match fixed(d, 72, |r| {
            let mut difficulty = [0; 11];
            for (k, x) in difficulty.iter_mut().enumerate() {
                *x = u32le(r, 20 + 4 * k)?;
            }
            Some(RttSubject { id: u32le(r, 0)?, name: sstr(r, 4, 16)?, short: sstr(r, 64, 4).unwrap_or_default(), difficulty, area: u32le(r, 68)? })
        }) {
            Ok(v) => l.subjects = v,
            Err(e) => l.warnings.push(format!("Предметы: {e}")),
        }
    }
    if let Some(d) = get("Area") {
        match fixed(d, 28, |r| Some(RttArea { id: u32le(r, 0)?, name: sstr(r, 4, 24)? })) {
            Ok(v) => l.areas = v,
            Err(e) => l.warnings.push(format!("Области: {e}")),
        }
    }
    if let Some(d) = get("Class") {
        match classes(d) {
            Ok(v) => l.classes = v,
            Err(e) => l.warnings.push(format!("Классы: {e}")),
        }
    }
    l.warnings.push("Учителя, нагрузки и сетка расписания из .rtt пока не переносятся (этап 2)".into());
    Ok(l)
}

fn grade_of(name: &str) -> u8 {
    name.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0)
}

/// Перенос справочников в новую школу.
pub fn to_school(l: &RttLists, name: &str) -> School {
    let area_name = |id: i32| l.areas.iter().find(|a| a.id == id).map(|a| a.name.clone()).unwrap_or_default();
    School {
        name: name.into(),
        settings: Settings::default(),
        buildings: vec![Building { id: 1, name: "Основной корпус".into(), address: String::new(), bells: vec![], travel_minutes: vec![] }],
        rooms: l
            .rooms
            .iter()
            .map(|r| Room {
                id: r.id as Id,
                name: r.name.clone(),
                building: 1,
                capacity: r.capacity.max(0) as u32,
                parallel_groups: 1,
                tags: vec![],
                availability: Availability::default(),
            })
            .collect(),
        subjects: l
            .subjects
            .iter()
            .map(|s| {
                let lvl = |r: std::ops::Range<usize>| s.difficulty[r].iter().copied().max().unwrap_or(0).clamp(0, 255) as u8;
                Subject {
                    id: s.id as Id,
                    name: s.name.clone(),
                    short: if s.short.is_empty() { s.name.chars().take(6).collect() } else { s.short.clone() },
                    area: area_name(s.area),
                    difficulty: [lvl(0..4), lvl(4..9), lvl(9..11)],
                }
            })
            .collect(),
        teachers: vec![],
        classes: l
            .classes
            .iter()
            .map(|c| Class {
                id: c.id as Id,
                name: c.name.clone(),
                grade: grade_of(&c.name),
                students: c.students.max(0) as u32,
                shift: c.shift.clamp(0, 9) as u8,
                building: None,
                max_per_day: c.max_per_day.clamp(1, 16) as u8,
                min_per_day: c.min_per_day.clamp(0, 16) as u8,
                availability: Availability::default(),
            })
            .collect(),
        loads: vec![],
    }
}
