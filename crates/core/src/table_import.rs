// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Загрузка таблиц из Excel и CSV: нагрузки школы и баллы трудности предметов.
//!
//! Нагрузки — самая долгая часть ввода данных в августе. Таблица с колонками
//! «Класс · Предмет · Учитель · Часов в неделю · Группа · Кабинет» превращается
//! в нагрузки; недостающие классы, предметы и учителя создаются сами.

use crate::model::*;
use std::collections::HashMap;

/// Прочитать первый лист таблицы: .xlsx, .xls, .ods или .csv.
pub fn read_table(bytes: &[u8], ext: &str) -> Result<Vec<Vec<String>>, String> {
    match ext.to_lowercase().as_str() {
        "csv" | "txt" => Ok(read_csv(bytes)),
        _ => read_sheet(bytes),
    }
}

fn read_sheet(bytes: &[u8]) -> Result<Vec<Vec<String>>, String> {
    use calamine::{open_workbook_auto_from_rs, Data, Reader};
    let mut wb = open_workbook_auto_from_rs(std::io::Cursor::new(bytes.to_vec())).map_err(|e| format!("не удалось открыть таблицу: {e}"))?;
    let name = wb.sheet_names().first().cloned().ok_or("в файле нет листов")?;
    let range = wb.worksheet_range(&name).map_err(|e| format!("не удалось прочитать лист «{name}»: {e}"))?;
    Ok(range
        .rows()
        .map(|row| {
            row.iter()
                .map(|c| match c {
                    Data::Empty => String::new(),
                    Data::Float(f) if f.fract() == 0.0 => format!("{}", *f as i64),
                    other => other.to_string().trim().to_string(),
                })
                .collect()
        })
        .collect())
}

fn read_csv(bytes: &[u8]) -> Vec<Vec<String>> {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t.to_string(),
        Err(_) => encoding_rs::WINDOWS_1251.decode(bytes).0.into_owned(),
    };
    // Разделитель — по первой строке с данными (не комментарию «#…»); при равенстве — «;».
    let first = text.lines().find(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#')).unwrap_or("");
    let sep = [',', '\t', ';'].into_iter().max_by_key(|c| first.matches(*c).count()).unwrap_or(';');
    text.lines().map(|l| split_csv(l, sep)).collect()
}

fn split_csv(line: &str, sep: char) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == sep && !quoted => out.push(std::mem::take(&mut cur).trim().to_string()),
            c => cur.push(c),
        }
    }
    out.push(cur.trim().to_string());
    out
}

/// Ключ для сравнения имён: без регистра, пробелов и точек, «ё» = «е»,
/// латинские буквы, похожие на русские, — русские («5A» = «5А»).
pub fn key(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '.' && *c != ',')
        .map(|c| match c {
            'ё' => 'е',
            'a' => 'а',
            'b' => 'б',
            'c' => 'с',
            'e' => 'е',
            'k' => 'к',
            'm' => 'м',
            'o' => 'о',
            'p' => 'р',
            'x' => 'х',
            'y' => 'у',
            other => other,
        })
        .collect()
}

/// Итог загрузки нагрузок.
#[derive(Debug, Default, Clone)]
pub struct LoadImport {
    pub loads: usize,
    pub hours: u32,
    pub new_classes: Vec<String>,
    pub new_subjects: Vec<String>,
    pub new_teachers: Vec<String>,
    /// Строки, которые пропущены или загружены не полностью, с номером строки таблицы.
    pub warnings: Vec<String>,
}

struct Cols {
    class: usize,
    subject: usize,
    teacher: usize,
    hours: usize,
    group: Option<usize>,
    room: Option<usize>,
}

fn find_header(rows: &[Vec<String>]) -> Option<(usize, Cols)> {
    for (i, row) in rows.iter().enumerate().take(20) {
        let low: Vec<String> = row.iter().map(|c| c.to_lowercase()).collect();
        let find = |w: &str| low.iter().position(|c| c.contains(w));
        if let (Some(class), Some(subject), Some(teacher), Some(hours)) = (find("класс"), find("предмет"), find("учител"), find("час")) {
            return Some((i, Cols { class, subject, teacher, hours, group: find("групп"), room: find("кабинет") }));
        }
    }
    None
}

fn next_id<T>(v: &[T], f: impl Fn(&T) -> Id) -> Id {
    v.iter().map(f).max().unwrap_or(0) + 1
}

fn teacher_keys(t: &Teacher) -> Vec<String> {
    let full = format!("{}{}{}", t.last_name, t.first_name, t.middle_name);
    let ini = format!(
        "{}{}{}",
        t.last_name,
        t.first_name.chars().next().map(String::from).unwrap_or_default(),
        t.middle_name.chars().next().map(String::from).unwrap_or_default()
    );
    vec![key(&full), key(&ini), key(&t.short_name())]
}

/// Загрузить нагрузки из строк таблицы. `replace` — сначала удалить все нагрузки школы
/// (уроки и замены по ним вызывающий удаляет сам).
pub fn import_loads(school: &mut School, rows: &[Vec<String>], replace: bool) -> Result<LoadImport, String> {
    let (head, cols) = find_header(rows).ok_or("не найдена строка заголовков: нужны колонки «Класс», «Предмет», «Учитель», «Часов в неделю»")?;
    let mut rep = LoadImport::default();
    if replace {
        school.loads.clear();
    }
    let cell = |row: &Vec<String>, i: usize| row.get(i).cloned().unwrap_or_default();
    // Группы одной нагрузки: одинаковые классы, предмет и часы, непустое название группы.
    let mut grouped: HashMap<(Vec<Id>, Id, u8), usize> = HashMap::new();
    for (n, row) in rows.iter().enumerate().skip(head + 1) {
        let line = n + 1;
        let (c, s, t, h) = (cell(row, cols.class), cell(row, cols.subject), cell(row, cols.teacher), cell(row, cols.hours));
        if c.is_empty() && s.is_empty() && t.is_empty() {
            continue;
        }
        if c.is_empty() || s.is_empty() || t.is_empty() {
            rep.warnings.push(format!("Строка {line}: не заполнены класс, предмет или учитель — пропущена"));
            continue;
        }
        let Some(hours) = h.replace(',', ".").parse::<f32>().ok().filter(|x| *x >= 1.0 && *x <= 40.0 && x.fract() == 0.0).map(|x| x as u8) else {
            rep.warnings.push(format!("Строка {line}: «{h}» — не целое число часов от 1 до 40, строка пропущена"));
            continue;
        };
        // Классы: «5а» или поток «5а, 5б».
        let mut class_ids = vec![];
        for name in c.split([',', ';', '+']).map(str::trim).filter(|x| !x.is_empty()) {
            let k = key(name);
            let id = match school.classes.iter().find(|x| key(&x.name) == k) {
                Some(x) => x.id,
                None => {
                    let id = next_id(&school.classes, |x| x.id);
                    let grade: u8 = name.chars().take_while(|ch| ch.is_ascii_digit()).collect::<String>().parse().unwrap_or(0);
                    let max = match grade {
                        1..=4 => 5,
                        5..=6 => 6,
                        _ => 7,
                    };
                    school.classes.push(Class {
                        id,
                        name: name.to_string(),
                        grade,
                        students: 25,
                        shift: 1,
                        building: school.buildings.first().map(|b| b.id),
                        max_per_day: max,
                        min_per_day: 0,
                        availability: Availability::default(),
                    });
                    rep.new_classes.push(name.to_string());
                    id
                }
            };
            if !class_ids.contains(&id) {
                class_ids.push(id);
            }
        }
        class_ids.sort();
        // Предмет — по названию или сокращению.
        let sk = key(&s);
        let subject = match school.subjects.iter().find(|x| key(&x.name) == sk || key(&x.short) == sk) {
            Some(x) => x.id,
            None => {
                let id = next_id(&school.subjects, |x| x.id);
                let short: String = s.chars().take(4).collect();
                school.subjects.push(Subject { id, name: s.clone(), short, area: String::new(), difficulty: [0; 3] });
                rep.new_subjects.push(s.clone());
                id
            }
        };
        // Учитель — по ФИО, «Фамилия И.О.» или единственной такой фамилии.
        let tk = key(&t);
        let last = key(t.split_whitespace().next().unwrap_or(""));
        let by_key = school.teachers.iter().find(|x| teacher_keys(x).contains(&tk)).map(|x| x.id);
        let by_last = || {
            let same: Vec<Id> = school.teachers.iter().filter(|x| key(&x.last_name) == last).map(|x| x.id).collect();
            (same.len() == 1 && t.split_whitespace().count() == 1).then(|| same[0])
        };
        let teacher = match by_key.or_else(by_last) {
            Some(id) => id,
            None => {
                let id = next_id(&school.teachers, |x| x.id);
                let mut parts = t.split_whitespace();
                let last_name = parts.next().unwrap_or("").to_string();
                let rest: Vec<&str> = parts.collect();
                let (first_name, middle_name) = match rest.as_slice() {
                    [] => (String::new(), String::new()),
                    [one] if one.matches('.').count() >= 2 => {
                        // «А.С.» — инициалы одним словом.
                        let ini: Vec<&str> = one.split('.').filter(|x| !x.is_empty()).collect();
                        (ini.first().map(|x| format!("{x}.")).unwrap_or_default(), ini.get(1).map(|x| format!("{x}.")).unwrap_or_default())
                    }
                    [one] => (one.to_string(), String::new()),
                    [a, b, ..] => (a.to_string(), b.to_string()),
                };
                school.teachers.push(Teacher {
                    id,
                    last_name,
                    first_name,
                    middle_name,
                    subjects: vec![],
                    home_room: None,
                    availability: Availability::default(),
                    ignore_windows: false,
                    methodical_days: 0,
                    max_substitutions_per_week: None,
                    can_substitute: true,
                    phone: String::new(),
                    note: String::new(),
                });
                rep.new_teachers.push(t.clone());
                id
            }
        };
        if let Some(tt) = school.teachers.iter_mut().find(|x| x.id == teacher) {
            if !tt.subjects.contains(&subject) {
                tt.subjects.push(subject);
            }
        }
        let label = cols.group.map(|i| cell(row, i)).unwrap_or_default();
        let rooms = match cols.room.map(|i| cell(row, i)).filter(|x| !x.is_empty()) {
            Some(r) => match school.rooms.iter().find(|x| key(&x.name) == key(&r)) {
                Some(x) => vec![x.id],
                None => {
                    rep.warnings.push(format!("Строка {line}: кабинета «{r}» нет в списке кабинетов — подберётся любой подходящий"));
                    vec![]
                }
            },
            None => vec![],
        };
        let group = Group { teacher, subject, rooms, label: label.clone(), students: None };
        let gk = (class_ids.clone(), subject, hours);
        if !label.is_empty() {
            if let Some(&idx) = grouped.get(&gk) {
                school.loads[idx].groups.push(group);
                continue;
            }
        }
        let id = next_id(&school.loads, |x| x.id);
        school.loads.push(Load {
            id,
            classes: class_ids,
            hours_per_week: hours,
            pairing: Pairing::default(),
            position: PositionReq::default(),
            groups: vec![group],
            paid: false,
        });
        if !label.is_empty() {
            grouped.insert(gk, school.loads.len() - 1);
        }
        rep.loads += 1;
        rep.hours += hours as u32;
    }
    if rep.loads == 0 && rep.warnings.is_empty() {
        return Err("в таблице нет строк с нагрузками под заголовком".into());
    }
    Ok(rep)
}

/// Шаблон таблицы нагрузок (.xlsx) с примером и пояснениями.
pub fn loads_template_xlsx() -> Result<Vec<u8>, String> {
    use rust_xlsxwriter::{Format, Workbook};
    let e = |x: rust_xlsxwriter::XlsxError| x.to_string();
    let mut wb = Workbook::new();
    let head = Format::new().set_bold().set_background_color(0xDDEBF7);
    let ws = wb.add_worksheet();
    ws.set_name("Нагрузки").map_err(e)?;
    let headers = ["Класс", "Предмет", "Учитель", "Часов в неделю", "Группа", "Кабинет"];
    let widths = [12.0, 24.0, 32.0, 16.0, 12.0, 12.0];
    for (i, (h, w)) in headers.iter().zip(widths).enumerate() {
        ws.write_string_with_format(0, i as u16, *h, &head).map_err(e)?;
        ws.set_column_width(i as u16, w).map_err(e)?;
    }
    let rows: [[&str; 6]; 5] = [
        ["5а", "Математика", "Иванова Анна Сергеевна", "5", "", "201"],
        ["5а", "Английский язык", "Петрова И.И.", "3", "1 группа", ""],
        ["5а", "Английский язык", "Сидорова О.В.", "3", "2 группа", ""],
        ["5а, 5б", "Физкультура", "Николаев А.И.", "3", "", "Спортзал"],
        ["5б", "Математика", "Иванова А.С.", "5", "", ""],
    ];
    for (r, row) in rows.iter().enumerate() {
        for (c, v) in row.iter().enumerate() {
            if c == 3 {
                ws.write_number(r as u32 + 1, c as u16, v.parse::<f64>().unwrap_or(0.0)).map_err(e)?;
            } else {
                ws.write_string(r as u32 + 1, c as u16, *v).map_err(e)?;
            }
        }
    }
    let help = wb.add_worksheet();
    help.set_name("Как заполнять").map_err(e)?;
    help.set_column_width(0, 110.0).map_err(e)?;
    let lines = [
        "Одна строка — один предмет в одном классе (или в потоке классов).",
        "Класс: «5а». Поток из нескольких классов — через запятую: «5а, 5б».",
        "Предмет: полное название или сокращение из списка предметов. Новые предметы создадутся сами.",
        "Учитель: «Иванова Анна Сергеевна» или «Иванова А.С.». Новые учителя создадутся сами, предмет добавится в их список.",
        "Часов в неделю: целое число.",
        "Группа: заполните, если класс делится. Строки с одинаковыми классом, предметом и часами и разными группами станут одной нагрузкой с группами.",
        "Кабинет: необязательно; название как в списке кабинетов.",
        "Первый лист должен называться как угодно, но заголовки — как в шаблоне. Строки-примеры удалите.",
    ];
    for (i, l) in lines.iter().enumerate() {
        help.write_string(i as u32, 0, *l).map_err(e)?;
    }
    wb.save_to_buffer().map_err(e)
}

/// Баллы трудности предметов в CSV (открывается в Excel).
pub fn difficulty_csv(school: &School) -> String {
    let mut out = format!(
        "\u{feff}# Баллы трудности предметов · профиль: {}\nПредмет;Сокращение;1–4 классы;5–9 классы;10–11 классы\n",
        school.settings.hygiene.name.replace(';', ",")
    );
    for s in &school.subjects {
        out.push_str(&format!("{};{};{};{};{}\n", s.name.replace(';', ","), s.short.replace(';', ","), s.difficulty[0], s.difficulty[1], s.difficulty[2]));
    }
    out
}

/// Загрузить баллы трудности из таблицы: строки «Предмет · Сокращение · 1–4 · 5–9 · 10–11».
/// Возвращает число обновлённых предметов и предупреждения.
pub fn apply_difficulty(school: &mut School, rows: &[Vec<String>]) -> Result<(usize, Vec<String>), String> {
    let mut updated = 0;
    let mut warnings = vec![];
    for (n, row) in rows.iter().enumerate() {
        let name = row.first().cloned().unwrap_or_default();
        if name.is_empty() || name.starts_with('#') || name.to_lowercase().starts_with("предмет") {
            continue;
        }
        let nums: Vec<Option<u8>> = (2..5).map(|i| row.get(i).and_then(|x| x.trim().parse::<u8>().ok()).filter(|x| *x <= 20)).collect();
        if nums.iter().any(|x| x.is_none()) {
            warnings.push(format!("Строка {}: баллы должны быть целыми числами от 0 до 20 — пропущена", n + 1));
            continue;
        }
        let k = key(&name);
        let short = row.get(1).map(|x| key(x)).unwrap_or_default();
        match school.subjects.iter_mut().find(|s| key(&s.name) == k || (!short.is_empty() && key(&s.short) == short)) {
            Some(s) => {
                s.difficulty = [nums[0].unwrap(), nums[1].unwrap(), nums[2].unwrap()];
                updated += 1;
            }
            None => warnings.push(format!("Строка {}: предмета «{name}» нет в списке — пропущена", n + 1)),
        }
    }
    if updated == 0 {
        return Err("ни один предмет из таблицы не найден в списке предметов".into());
    }
    Ok((updated, warnings))
}
