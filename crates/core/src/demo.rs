// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Демонстрационная школа: два корпуса, 13 классов, деление на группы,
//! общий спортзал на две группы, учителя, работающие в обоих зданиях.
//! Баллы трудности — демонстрационные, сверяйте с действующим СанПиН.

use crate::model::*;

struct Spec {
    name: &'static str,
    short: &'static str,
    area: &'static str,
    diff: [u8; 3],
    specialty: &'static str,
    room_tag: Option<&'static str>,
}

const SUBJECTS: &[Spec] = &[
    Spec { name: "Русский язык", short: "Рус", area: "Филология", diff: [7, 7, 9], specialty: "rus", room_tag: None },
    Spec { name: "Литература", short: "Лит", area: "Филология", diff: [5, 4, 4], specialty: "rus", room_tag: None },
    Spec {
        name: "Литературное чтение", short: "Лит.чт", area: "Филология", diff: [5, 5, 5], specialty: "prim", room_tag: None
    },
    Spec { name: "Математика", short: "Мат", area: "Математика", diff: [8, 10, 10], specialty: "math", room_tag: None },
    Spec { name: "Алгебра", short: "Алг", area: "Математика", diff: [0, 9, 10], specialty: "math", room_tag: None },
    Spec { name: "Геометрия", short: "Геом", area: "Математика", diff: [0, 9, 11], specialty: "math", room_tag: None },
    Spec {
        name: "Информатика", short: "Инф", area: "Математика", diff: [0, 7, 6], specialty: "inf", room_tag: Some("информатика")
    },
    Spec { name: "Английский язык", short: "Англ", area: "Филология", diff: [7, 9, 8], specialty: "eng", room_tag: None },
    Spec { name: "Окружающий мир", short: "Окр", area: "Естествознание", diff: [6, 6, 6], specialty: "prim", room_tag: None },
    Spec { name: "История", short: "Ист", area: "Обществознание", diff: [0, 5, 5], specialty: "hist", room_tag: None },
    Spec { name: "Обществознание", short: "Общ", area: "Обществознание", diff: [0, 6, 5], specialty: "hist", room_tag: None },
    Spec { name: "География", short: "Гео", area: "Естествознание", diff: [0, 6, 3], specialty: "geo", room_tag: None },
    Spec { name: "Биология", short: "Био", area: "Естествознание", diff: [0, 7, 7], specialty: "bio", room_tag: None },
    Spec { name: "Физика", short: "Физ", area: "Естествознание", diff: [0, 9, 9], specialty: "phys", room_tag: None },
    Spec { name: "Химия", short: "Хим", area: "Естествознание", diff: [0, 10, 11], specialty: "bio", room_tag: Some("химия") },
    Spec { name: "ИЗО", short: "ИЗО", area: "Искусство", diff: [3, 3, 3], specialty: "art", room_tag: None },
    Spec { name: "Музыка", short: "Муз", area: "Искусство", diff: [3, 2, 2], specialty: "art", room_tag: None },
    Spec { name: "Технология", short: "Техн", area: "Технология", diff: [2, 4, 4], specialty: "art", room_tag: None },
    Spec {
        name: "Физическая культура", short: "Физ-ра", area: "Физическая культура", diff: [1, 2, 2], specialty: "pe", room_tag: Some("спортзал")
    },
];

fn subj(name: &str) -> Id {
    SUBJECTS.iter().position(|s| s.short == name).expect(name) as Id + 1
}

fn curriculum(grade: u8) -> Vec<(&'static str, u8)> {
    match grade {
        1 => vec![("Рус", 5), ("Мат", 4), ("Лит.чт", 4), ("Окр", 2), ("ИЗО", 1), ("Муз", 1), ("Техн", 1), ("Физ-ра", 3)],
        2..=4 => vec![("Рус", 5), ("Мат", 4), ("Лит.чт", 4), ("Англ", 2), ("Окр", 2), ("ИЗО", 1), ("Муз", 1), ("Техн", 1), ("Физ-ра", 3)],
        5 | 6 => {
            let mut v =
                vec![("Рус", 5), ("Лит", 3), ("Мат", 5), ("Англ", 3), ("Ист", 2), ("Био", 1), ("Гео", 1), ("ИЗО", 1), ("Муз", 1), ("Техн", 2), ("Физ-ра", 2)];
            if grade == 6 {
                v.push(("Общ", 1));
            }
            v
        }
        7..=9 => {
            let mut v = vec![
                ("Рус", 4),
                ("Лит", 2),
                ("Алг", 3),
                ("Геом", 2),
                ("Инф", 1),
                ("Англ", 3),
                ("Ист", 2),
                ("Общ", 1),
                ("Гео", 2),
                ("Био", 2),
                ("Физ", 2),
                ("Физ-ра", 2),
            ];
            if grade >= 8 {
                v.push(("Хим", 2));
            } else {
                v.push(("Техн", 1));
            }
            v
        }
        _ => vec![
            ("Рус", 2),
            ("Лит", 3),
            ("Алг", 4),
            ("Геом", 3),
            ("Инф", 1),
            ("Англ", 3),
            ("Ист", 2),
            ("Общ", 2),
            ("Физ", 2),
            ("Хим", 2),
            ("Био", 2),
            ("Физ-ра", 2),
        ],
    }
}

const SURNAMES: &[&str] = &[
    "Иванова",
    "Петров",
    "Смирнова",
    "Кузнецов",
    "Попова",
    "Васильев",
    "Соколова",
    "Михайлов",
    "Новикова",
    "Фёдоров",
    "Морозова",
    "Волков",
    "Алексеева",
    "Лебедев",
    "Семёнова",
    "Егоров",
    "Павлова",
    "Козлов",
    "Степанова",
    "Николаев",
    "Орлова",
    "Андреев",
    "Макарова",
    "Захаров",
    "Зайцева",
    "Соловьёв",
    "Борисова",
    "Яковлев",
    "Григорьева",
    "Романов",
];
const NAMES: &[(&str, &str)] = &[
    ("Анна", "Сергеевна"),
    ("Игорь", "Петрович"),
    ("Елена", "Викторовна"),
    ("Олег", "Николаевич"),
    ("Мария", "Андреевна"),
    ("Дмитрий", "Ильич"),
    ("Ольга", "Павловна"),
    ("Сергей", "Юрьевич"),
    ("Татьяна", "Олеговна"),
    ("Алексей", "Иванович"),
];

pub fn demo_school() -> School {
    let mut settings = Settings::default();
    settings.days_per_week = 5;
    settings.periods_per_day = 7;
    settings.hygiene = HygieneProfile {
        name: "Демо: пик во вторник/среду (сверьте с СанПиН 1.2.3685-21)".into(), peak_days: vec![1, 2], light_days: vec![]
    };
    let bells = |start_h: u32, start_m: u32, len: u32, brk: &[u32]| -> Vec<BellTime> {
        let mut t = start_h * 60 + start_m;
        (0..7u8)
            .map(|p| {
                let b = BellTime { period: p, start: format!("{:02}:{:02}", t / 60, t % 60), end: format!("{:02}:{:02}", (t + len) / 60, (t + len) % 60) };
                t += len + brk.get(p as usize).copied().unwrap_or(10);
                b
            })
            .collect()
    };
    let buildings = vec![
        Building {
            id: 1,
            name: "Главный корпус".into(),
            address: "ул. Школьная, 1".into(),
            bells: bells(8, 30, 45, &[10, 20, 20, 10, 10, 10]),
            travel_minutes: vec![(2, 12)],
        },
        Building {
            id: 2,
            name: "Начальная школа".into(),
            address: "ул. Садовая, 7".into(),
            bells: bells(8, 30, 40, &[15, 20, 20, 15, 10, 10]),
            travel_minutes: vec![(1, 12)],
        },
    ];
    let mut rooms = vec![];
    let mut rid = 1;
    for n in 101..=112 {
        rooms.push(Room { id: rid, name: n.to_string(), building: 1, capacity: 30, parallel_groups: 1, tags: vec![], availability: Availability::default() });
        rid += 1;
    }
    for (name, tag, cap, par) in [("Спортзал", "спортзал", 60, 2u8), ("Информатика", "информатика", 30, 1), ("Химия", "химия", 30, 1)]
    {
        rooms.push(Room {
            id: rid,
            name: name.into(),
            building: 1,
            capacity: cap,
            parallel_groups: par,
            tags: vec![tag.into()],
            availability: Availability::default(),
        });
        rid += 1;
    }
    for n in 201..=206 {
        rooms.push(Room { id: rid, name: n.to_string(), building: 2, capacity: 28, parallel_groups: 1, tags: vec![], availability: Availability::default() });
        rid += 1;
    }
    rooms.push(Room {
        id: rid,
        name: "Спортзал НШ".into(),
        building: 2,
        capacity: 30,
        parallel_groups: 1,
        tags: vec!["спортзал".into()],
        availability: Availability::default(),
    });
    let room_by_tag = |tag: &str, b: Id| rooms.iter().find(|r| r.building == b && r.tags.iter().any(|t| t == tag)).map(|r| r.id);

    let subjects: Vec<Subject> = SUBJECTS
        .iter()
        .enumerate()
        .map(|(i, s)| Subject { id: i as Id + 1, name: s.name.into(), short: s.short.into(), area: s.area.into(), difficulty: s.diff })
        .collect();

    let class_list: &[(&str, u8, u32)] = &[
        ("1а", 1, 25),
        ("1б", 1, 24),
        ("2а", 2, 26),
        ("3а", 3, 27),
        ("4а", 4, 25),
        ("5а", 5, 28),
        ("5б", 5, 26),
        ("6а", 6, 27),
        ("7а", 7, 25),
        ("8а", 8, 26),
        ("9а", 9, 24),
        ("10а", 10, 22),
        ("11а", 11, 20),
    ];
    let classes: Vec<Class> = class_list
        .iter()
        .enumerate()
        .map(|(i, (n, g, st))| Class {
            id: i as Id + 1,
            name: (*n).into(),
            grade: *g,
            students: *st,
            shift: 1,
            building: Some(if *g <= 4 { 2 } else { 1 }),
            max_per_day: match g {
                1 => 5,
                2..=4 => 5,
                5..=6 => 6,
                _ => 7,
            },
            min_per_day: if *g <= 4 { 4 } else { 5 },
            availability: Availability::default(),
        })
        .collect();

    // Учителя: по одному «классному» на каждый начальный класс + предметники.
    let mut teachers: Vec<Teacher> = vec![];
    let mut name_i = 0usize;
    let mut new_teacher = |subjects: Vec<Id>, home: Option<Id>, teachers: &mut Vec<Teacher>| -> Id {
        let id = teachers.len() as Id + 1;
        let (f, m) = NAMES[name_i % NAMES.len()];
        let last = SURNAMES[name_i % SURNAMES.len()];
        name_i += 1;
        teachers.push(Teacher {
            id,
            last_name: last.into(),
            first_name: f.into(),
            middle_name: m.into(),
            subjects,
            home_room: home,
            availability: Availability::default(),
            ignore_windows: false,
            methodical_days: 0,
            max_substitutions_per_week: Some(6),
            can_substitute: true,
            phone: String::new(),
            note: String::new(),
        });
        id
    };
    let prim_subjects: Vec<Id> = ["Рус", "Мат", "Лит.чт", "Окр", "ИЗО", "Техн"].iter().map(|s| subj(s)).collect();
    let mut prim_teacher = std::collections::HashMap::new();
    for c in classes.iter().filter(|c| c.grade <= 4) {
        let home = 200 + c.id; // 201..205 → id кабинета
        let home_id = rooms.iter().find(|r| r.name == home.to_string()).map(|r| r.id);
        prim_teacher.insert(c.id, (new_teacher(prim_subjects.clone(), home_id, &mut teachers), home_id));
    }
    // предметники: specialty -> список учителей с остатком часов
    let specialties: &[(&str, &[&str], usize)] = &[
        ("rus", &["Рус", "Лит"], 2),
        ("math", &["Мат", "Алг", "Геом"], 2),
        ("eng", &["Англ"], 3),
        ("hist", &["Ист", "Общ"], 1),
        ("geo", &["Гео"], 1),
        ("bio", &["Био", "Хим"], 1),
        ("phys", &["Физ", "Инф"], 1),
        ("inf", &["Инф", "Алг"], 1),
        ("art", &["ИЗО", "Муз", "Техн"], 2),
        ("pe", &["Физ-ра"], 3),
    ];
    let mut spec_teachers: std::collections::HashMap<&str, Vec<(Id, i32)>> = Default::default();
    for (sp, subs, n) in specialties {
        for _ in 0..*n {
            let id = new_teacher(subs.iter().map(|s| subj(s)).collect(), None, &mut teachers);
            spec_teachers.entry(sp).or_default().push((id, 26));
        }
    }
    // завуч без ограничений на окна, тоже может заменять
    let zavuch = new_teacher(vec![subj("Мат"), subj("Алг")], None, &mut teachers);
    teachers[zavuch as usize - 1].ignore_windows = true;
    teachers[zavuch as usize - 1].note = "Завуч".into();
    spec_teachers.get_mut("math").unwrap().push((zavuch, 6));

    let mut loads = vec![];
    let mut lid = 1;
    let take = |sp: &str, hours: i32, prefer: Option<Id>, st: &mut std::collections::HashMap<&str, Vec<(Id, i32)>>| -> Id {
        let v = st.get_mut(sp).unwrap();
        if let Some(p) = prefer {
            if let Some(e) = v.iter_mut().find(|e| e.0 == p && e.1 >= hours) {
                e.1 -= hours;
                return e.0;
            }
        }
        v.sort_by_key(|e| -e.1);
        v[0].1 -= hours;
        v[0].0
    };
    for c in &classes {
        let b = c.building.unwrap();
        // учитель предмета, закреплённый за классом (один учитель на «рус+лит» и т.п.)
        let mut class_spec: std::collections::HashMap<&str, Id> = Default::default();
        for (short, h) in curriculum(c.grade) {
            let spec = SUBJECTS.iter().find(|s| s.short == short).unwrap();
            let sid = subj(short);
            let pairing = if short == "Физ-ра" || short == "Технология" {
                Pairing::NoDouble
            } else if h >= 4 && c.grade >= 7 && (short == "Алг" || short == "Рус") {
                Pairing::Any
            } else if h <= 2 {
                Pairing::NoDouble
            } else {
                Pairing::Any
            };
            let tag_room = spec.room_tag.and_then(|t| room_by_tag(t, b));
            if c.grade <= 4 && spec.specialty == "prim" || (c.grade <= 4 && ["Рус", "Мат", "ИЗО", "Техн"].contains(&short)) {
                let (t, home) = prim_teacher[&c.id];
                loads.push(Load {
                    id: lid,
                    classes: vec![c.id],
                    hours_per_week: h,
                    pairing,
                    position: PositionReq::Any,
                    groups: vec![Group { teacher: t, subject: sid, rooms: home.into_iter().collect(), label: String::new(), students: None }],
                    paid: false,
                });
                lid += 1;
                continue;
            }
            let sp = if spec.specialty == "prim" { "rus" } else { spec.specialty };
            if short == "Англ" && c.grade >= 5 {
                let t1 = take("eng", h as i32, None, &mut spec_teachers);
                let t2 = loop {
                    let t = take("eng", h as i32, None, &mut spec_teachers);
                    if t != t1 {
                        break t;
                    }
                };
                let half = c.students / 2;
                loads.push(Load {
                    id: lid,
                    classes: vec![c.id],
                    hours_per_week: h,
                    pairing,
                    position: PositionReq::Any,
                    groups: vec![
                        Group { teacher: t1, subject: sid, rooms: vec![], label: "1 гр.".into(), students: Some(half) },
                        Group { teacher: t2, subject: sid, rooms: vec![], label: "2 гр.".into(), students: Some(c.students - half) },
                    ],
                    paid: false,
                });
                lid += 1;
                continue;
            }
            let prefer = class_spec.get(sp).copied();
            let t = take(sp, h as i32, prefer, &mut spec_teachers);
            class_spec.insert(sp, t);
            let mut room_list: Vec<Id> = tag_room.into_iter().collect();
            if short == "Физ-ра" && c.grade <= 4 {
                room_list = room_by_tag("спортзал", 2).into_iter().collect();
            }
            loads.push(Load {
                id: lid,
                classes: vec![c.id],
                hours_per_week: h,
                pairing,
                position: PositionReq::Any,
                groups: vec![Group { teacher: t, subject: sid, rooms: room_list, label: String::new(), students: None }],
                paid: false,
            });
            lid += 1;
        }
    }
    // Пожелания: методический день у двух учителей, «нежелательные» 7-е уроки у пары.
    if let Some(t) = teachers.iter_mut().find(|t| t.subjects.contains(&subj("Гео"))) {
        t.availability.days_off = vec![4]; // пятница
    }
    if let Some(t) = teachers.iter_mut().find(|t| t.subjects.contains(&subj("Ист"))) {
        t.methodical_days = 1;
    }
    for t in teachers.iter_mut().filter(|t| t.subjects.contains(&subj("ИЗО"))) {
        t.availability.if_needed = (0..5).map(|d| Slot::new(d, 6)).collect();
    }
    School { name: "МБОУ «Демо-школа №1» (два корпуса)".into(), settings, buildings, rooms, subjects, teachers, classes, loads }
}
