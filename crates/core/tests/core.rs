// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
use chrono::NaiveDate;
use skyurok_core::substitute::{auto_plan, check_replacements, SubContext, SubPolicy};
use skyurok_core::validate::{analyze, precheck, Severity};
use skyurok_core::*;
use std::time::Duration;

fn solved() -> Project {
    let mut p = Project::new(demo::demo_school());
    let r = solver::solve(
        &p.school,
        &solver::SolveOptions { seed: 3, max_iterations: 200_000, time_limit: Duration::from_secs(5), keep_pinned_from: None, ..Default::default() },
    );
    p.timetable = r.timetable;
    p
}

#[test]
fn demo_data_is_consistent() {
    let s = demo::demo_school();
    let errs: Vec<_> = precheck(&s).into_iter().filter(|i| i.severity == Severity::Error).collect();
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn solver_respects_hard_constraints() {
    let p = solved();
    let rep = analyze(&p.school, &p.timetable);
    let hard: Vec<_> = rep.issues.iter().filter(|i| i.severity == Severity::Error && i.code != "unplaced").collect();
    assert!(hard.is_empty(), "{hard:#?}");
    assert_eq!(rep.stats.unplaced, 0);
}

#[test]
fn solver_is_deterministic() {
    let s = demo::demo_school();
    let o = solver::SolveOptions { seed: 11, max_iterations: 20_000, time_limit: Duration::from_secs(30), keep_pinned_from: None, ..Default::default() };
    let a = solver::solve(&s, &o);
    let b = solver::solve(&s, &o);
    assert_eq!(serde_json::to_string(&a.timetable).unwrap(), serde_json::to_string(&b.timetable).unwrap());
}

#[test]
fn pinned_lessons_stay() {
    let mut p = solved();
    for l in p.timetable.lessons.iter_mut().take(10) {
        l.pinned = true;
    }
    let pinned: Vec<(Id, Slot)> = p.timetable.lessons.iter().take(10).map(|l| (l.load, l.slot)).collect();
    let r = solver::solve(
        &p.school,
        &solver::SolveOptions {
            seed: 5,
            max_iterations: 50_000,
            time_limit: Duration::from_secs(5),
            keep_pinned_from: Some(p.timetable.clone()),
            ..Default::default()
        },
    );
    for (load, slot) in pinned {
        assert!(r.timetable.lessons.iter().any(|l| l.load == load && l.slot == slot && l.pinned));
    }
}

fn first_school_day(p: &Project, teacher: Id) -> NaiveDate {
    // понедельник 09.11.2026 + день, в который у учителя есть уроки
    let day = p
        .timetable
        .lessons
        .iter()
        .find(|l| p.school.loads.iter().any(|ld| ld.id == l.load && ld.groups.iter().any(|g| g.teacher == teacher)))
        .unwrap()
        .slot
        .day;
    NaiveDate::from_ymd_opt(2026, 11, 9).unwrap() + chrono::Days::new(day as u64)
}

#[test]
fn suggestions_never_offer_busy_teachers() {
    let mut p = solved();
    let t = p.school.loads[20].groups[0].teacher;
    let date = first_school_day(&p, t);
    p.absences.push(Absence { id: 1, teacher: t, from: date, to: date, reason: String::new(), periods: None });
    let ctx = SubContext::new(&p.school, &p.timetable, &p.absences).with_classes(&p.class_absences);
    let affected = ctx.affected(date, date);
    assert!(!affected.is_empty());
    for a in &affected {
        for s in ctx.suggest(a, &[], &SubPolicy::standard()) {
            if let ReplacementKind::Substitute { teacher, .. } = s.kind {
                assert_ne!(teacher, t);
                assert!(ctx.busy(teacher, date, a.slot, &[]).is_none());
            }
        }
    }
}

#[test]
fn auto_plan_has_no_conflicts_and_covers_everything() {
    let mut p = solved();
    let t1 = p.school.loads[30].groups[0].teacher;
    let t2 = p.school.loads[45].groups[0].teacher;
    let from = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
    let to = NaiveDate::from_ymd_opt(2026, 11, 13).unwrap();
    for (i, t) in [t1, t2].into_iter().enumerate() {
        p.absences.push(Absence { id: i as Id + 1, teacher: t, from, to, reason: String::new(), periods: None });
    }
    let ctx = SubContext::new(&p.school, &p.timetable, &p.absences).with_classes(&p.class_absences);
    let mut next = 1;
    let plan = auto_plan(&ctx, &[], from, to, &SubPolicy::standard(), &mut next);
    let affected = ctx.affected(from, to).len();
    assert_eq!(plan.replacements.len() + plan.unresolved.len(), affected);
    let issues: Vec<_> = check_replacements(&ctx, &plan.replacements).into_iter().filter(|i| i.severity == Severity::Error).collect();
    assert!(issues.is_empty(), "{issues:#?}");
}

#[test]
fn double_booking_in_replacements_is_detected() {
    let p = solved();
    let ctx = SubContext::new(&p.school, &p.timetable, &p.absences).with_classes(&p.class_absences);
    let l = &p.timetable.lessons[0];
    let load = p.school.loads.iter().find(|x| x.id == l.load).unwrap();
    // заменяющий ведёт в это время свой урок
    let other = p.timetable.lessons.iter().find(|o| o.slot == l.slot && o.load != l.load).unwrap();
    let busy_t = p.school.loads.iter().find(|x| x.id == other.load).unwrap().groups[0].teacher;
    let date = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap() + chrono::Days::new(l.slot.day as u64);
    let r = Replacement {
        id: 1,
        date,
        lesson: l.id,
        group: 0,
        original_teacher: load.groups[0].teacher,
        kind: ReplacementKind::Substitute { teacher: busy_t, subject: load.groups[0].subject },
        room: None,
        note: String::new(),
        confirmed: true,
        anchor: None,
        stale: false,
    };
    let issues = check_replacements(&ctx, &[r.clone(), Replacement { id: 2, ..r }]);
    let codes: Vec<&str> = issues.iter().map(|i| i.code).collect();
    assert!(codes.contains(&"rep_teacher_busy"));
    assert!(codes.contains(&"rep_duplicate"));
    assert!(codes.contains(&"rep_no_absence"));
}

#[test]
fn project_roundtrip_and_exports() {
    let p = solved();
    let back = Project::load_json(&p.to_json()).unwrap();
    assert_eq!(back.timetable.lessons.len(), p.timetable.lessons.len());
    let d = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
    assert!(export::html_classes(&p, None).contains("Расписание 5а класса"));
    assert!(export::csv_timetable(&p).lines().count() > p.timetable.lessons.len());
    assert!(export::html_day(&p, d).contains("Расписание на 10.11.2026"));
}

#[test]
fn rtt_sections_and_fixed_records() {
    // синтетический файл: [General] + [Room] с одной записью
    let mut b = b"[General]".to_vec();
    b.extend([7, 0, 0, 0]);
    b.extend(b"[Room]");
    b.extend(1i32.to_le_bytes());
    let mut rec = vec![0u8; 36];
    rec[0..4].copy_from_slice(&42i32.to_le_bytes());
    rec[4] = 3;
    rec[5..8].copy_from_slice(&[0xd1, 0xef, 0xee]); // «Спо» в cp1251
    rec[20..24].copy_from_slice(&30i32.to_le_bytes());
    b.extend(rec);
    let l = rtt::read_lists(&b).unwrap();
    assert_eq!(l.rooms.len(), 1);
    assert_eq!(l.rooms[0].name, "Спо");
    assert_eq!(l.rooms[0].capacity, 30);
}

#[test]
fn word_and_pdf_export_with_draft() {
    use skyurok_core::printdoc::{build, PrintOptions, Section};
    let mut p = solved();
    // создаём конфликт: второй урок того же учителя в тот же слот
    let a = p.timetable.lessons[0].clone();
    let t = p.school.loads.iter().find(|l| l.id == a.load).unwrap().groups[0].teacher;
    let other = p
        .timetable
        .lessons
        .iter()
        .position(|l| l.id != a.id && l.slot != a.slot && p.school.loads.iter().find(|x| x.id == l.load).unwrap().groups[0].teacher == t)
        .unwrap();
    p.timetable.lessons[other].slot = a.slot;
    let d = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
    let mut o = PrintOptions::new(d, d);
    o.draft = true;
    let doc = build(&p, &[Section::Classes, Section::School], &o);
    assert!(doc.draft.as_deref().unwrap().starts_with("ЧЕРНОВИК"));
    assert_eq!(doc.pages.last().unwrap().title, "Проблемы в расписании");
    let docx = export_docx::write(&doc).unwrap();
    assert_eq!(&docx[..2], b"PK");
    let pdf = export_pdf::write(&doc).unwrap();
    assert_eq!(&pdf[..5], b"%PDF-");
    // без пометки — чистовик без листа проблем
    o.draft = false;
    let clean = build(&p, &[Section::Classes], &o);
    assert!(clean.draft.is_none());
    assert_ne!(clean.pages.last().unwrap().title, "Проблемы в расписании");
}

#[test]
fn class_absence_frees_teacher_and_skips_lessons() {
    let mut p = solved();
    let l = p.timetable.lessons[0].clone();
    let load = p.school.loads.iter().find(|x| x.id == l.load).unwrap().clone();
    let teacher = load.groups[0].teacher;
    let date = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap() + chrono::Days::new(l.slot.day as u64);
    p.class_absences.push(ClassAbsence { id: 1, class: load.classes[0], from: date, to: date, reason: "экскурсия".into(), periods: None });
    let ctx = SubContext::new(&p.school, &p.timetable, &p.absences).with_classes(&p.class_absences);
    assert!(ctx.busy(teacher, date, l.slot, &[]).is_none());
    p.absences.push(Absence { id: 1, teacher, from: date, to: date, reason: String::new(), periods: None });
    let ctx = SubContext::new(&p.school, &p.timetable, &p.absences).with_classes(&p.class_absences);
    assert!(ctx.affected(date, date).iter().all(|a| a.lesson_id != l.id));
}

/// Проект с автоподобранными заменами на неделю для одного отсутствующего учителя.
fn with_replacements() -> (Project, NaiveDate) {
    let mut p = solved();
    let t = p.school.teachers[0].id;
    let from = first_school_day(&p, t);
    let to = from + chrono::Days::new(4);
    p.absences.push(Absence { id: 1, teacher: t, from, to, reason: String::new(), periods: None });
    let ctx = SubContext::new(&p.school, &p.timetable, &p.absences).with_classes(&p.class_absences);
    let mut next = 1;
    let plan = auto_plan(&ctx, &[], from, to, &SubPolicy::standard(), &mut next);
    assert!(!plan.replacements.is_empty());
    p.replacements = plan.replacements;
    p.normalize();
    (p, from)
}

fn broken_links(p: &Project) -> Vec<String> {
    let ctx = SubContext::new(&p.school, &p.timetable, &p.absences).with_classes(&p.class_absences);
    check_replacements(&ctx, &p.replacements).into_iter().filter(|i| i.code == "rep_bad_lesson" || i.code == "rep_wrong_day").map(|i| i.message).collect()
}

#[test]
fn resolving_keeps_replacements_attached() {
    let (mut p, from) = with_replacements();
    assert!(p.replacements.iter().all(|r| r.anchor.is_some()));
    let n = p.replacements.len();
    let other = solver::solve(&p.school, &solver::SolveOptions { seed: 99, max_iterations: 100_000, time_limit: Duration::from_secs(5), ..Default::default() });
    // Без переноса замены «отрываются» от уроков — так было в 1.0.
    let mut naive = p.clone();
    naive.timetable = other.timetable.clone();
    assert!(!broken_links(&naive).is_empty(), "ожидали, что без переноса замены сломаются");
    // С переносом — ни одной оторванной замены.
    let today = from - chrono::Days::new(1);
    let rep = history::apply_timetable(&mut p, other.timetable, today, "2026-11-08 12:00", "тест");
    assert_eq!(rep.kept + rep.shifted + rep.removed + rep.archived, n);
    assert_eq!(rep.archived, 0);
    assert!(broken_links(&p).is_empty(), "{:?}", broken_links(&p));
    assert_eq!(p.replacements.iter().filter(|r| r.stale).count(), rep.shifted);
    assert_eq!(p.versions.len(), 1, "старое расписание сохранено как версия");
}

#[test]
fn past_replacements_stay_in_history_and_print() {
    let (mut p, from) = with_replacements();
    let n = p.replacements.len();
    let other = solver::solve(&p.school, &solver::SolveOptions { seed: 7, max_iterations: 100_000, time_limit: Duration::from_secs(5), ..Default::default() });
    let today = from + chrono::Days::new(30);
    let rep = history::apply_timetable(&mut p, other.timetable, today, "2026-11-08 12:00", "тест");
    assert_eq!(rep.removed, 0);
    assert_eq!(p.replacements.len(), n, "история не теряется");
    assert!(broken_links(&p).is_empty());
    let html = export::html_replacements(&p, from, from + chrono::Days::new(4));
    let rows = html.matches("<tr class=").count();
    assert_eq!(rows, n, "все замены из истории попадают в лист");
}

#[test]
fn versions_restore_and_auto_limit() {
    let (mut p, from) = with_replacements();
    let original = p.timetable.clone();
    let vid = history::save_version(&mut p, "Утверждённое", "2026-09-01 09:00", false);
    for i in 0..15 {
        let r =
            solver::solve(&p.school, &solver::SolveOptions { seed: 100 + i, max_iterations: 2_000, time_limit: Duration::from_secs(5), ..Default::default() });
        history::apply_timetable(&mut p, r.timetable, from, "2026-11-08 12:00", "авто");
    }
    assert_eq!(p.versions.iter().filter(|v| v.auto).count(), history::AUTO_VERSIONS_KEEP);
    assert!(p.versions.iter().any(|v| v.id == vid), "ручная версия не удаляется");
    history::restore_version(&mut p, vid, from, "2026-11-08 13:00").unwrap();
    assert_eq!(p.timetable, original);
    assert!(broken_links(&p).is_empty());
}

#[test]
fn format_1_files_open_and_get_anchors() {
    let (p, _) = with_replacements();
    let mut v: serde_json::Value = serde_json::from_str(&p.to_json()).unwrap();
    v["format_version"] = 1.into();
    v.as_object_mut().unwrap().remove("versions");
    for r in v["replacements"].as_array_mut().unwrap() {
        r.as_object_mut().unwrap().remove("anchor");
    }
    let q = Project::load_json(&v.to_string()).unwrap();
    assert_eq!(q.format_version, model::PROJECT_FORMAT_VERSION);
    assert!(q.replacements.iter().all(|r| r.anchor.is_some()));
    let mut newer = v.clone();
    newer["format_version"] = 99.into();
    assert!(Project::load_json(&newer.to_string()).unwrap_err().contains("Обновите программу"));
}

#[test]
fn phone_page_and_qr_on_sheet() {
    let (mut p, from) = with_replacements();
    p.absences[0].reason = "больничный".into();
    p.school.settings.public_url = "https://school.example/zameny.html".into();
    let html = export::html_phone(&p, from, 2, "08.11.2026 07:30");
    let first_day = p.replacements.iter().filter(|r| r.date == from).count();
    assert!(html.matches("class=\"card").count() >= first_day);
    assert!(html.contains("viewport"));
    assert!(!html.contains("больничный"), "причина отсутствия не публикуется");
    let sheet = export::html_replacements(&p, from, from);
    assert!(sheet.contains("<svg") && !sheet.contains("больничный"));
    let m = qr::matrix(&p.school.settings.public_url).unwrap();
    assert!(m.size >= 21 && m.dark.iter().any(|d| *d));
    let opts = printdoc::PrintOptions::new(from, from);
    let doc = printdoc::build(&p, &[printdoc::Section::Replacements], &opts);
    assert!(doc.pages.iter().any(|pg| pg.qr.is_some()));
    assert!(!doc.pages.iter().any(|pg| pg.note.as_deref().unwrap_or("").contains("больничный")));
    let pdf = export_pdf::write(&doc).unwrap();
    assert!(pdf.len() > 10_000);
    let docx = export_docx::write(&doc).unwrap();
    assert!(!docx.is_empty());
}

#[test]
fn loads_from_excel_template() {
    let bytes = table_import::loads_template_xlsx().unwrap();
    let rows = table_import::read_table(&bytes, "xlsx").unwrap();
    assert_eq!(rows[0][0], "Класс");
    let mut s = School {
        name: "Тест".into(),
        settings: Default::default(),
        buildings: vec![model::Building { id: 1, name: "Здание".into(), address: String::new(), bells: vec![], travel_minutes: vec![] }],
        rooms: vec![model::Room { id: 1, name: "201".into(), ..demo::demo_school().rooms[0].clone() }],
        subjects: vec![],
        teachers: vec![],
        classes: vec![],
        loads: vec![],
    };
    let r = table_import::import_loads(&mut s, &rows, false).unwrap();
    // 5 строк: английский в 5а — одна нагрузка с двумя группами, физкультура — поток 5а+5б.
    assert_eq!(r.loads, 4, "{r:?}");
    assert_eq!(s.classes.len(), 2);
    let eng = s.loads.iter().find(|l| l.groups.len() == 2).expect("нагрузка с группами");
    assert_eq!(eng.hours_per_week, 3);
    assert!(s.loads.iter().any(|l| l.classes.len() == 2), "поток 5а, 5б");
    // «Иванова Анна Сергеевна» и «Иванова А.С.» — один учитель.
    assert_eq!(s.teachers.iter().filter(|t| t.last_name == "Иванова").count(), 1);
    assert!(s.loads[0].groups[0].rooms == vec![1], "кабинет 201 найден");
    assert!(r.warnings.iter().any(|w| w.contains("Спортзал")));
    let t = s.teachers.iter().find(|t| t.last_name == "Иванова").unwrap();
    assert!(!t.subjects.is_empty(), "предмет добавлен учителю");
    // CSV с точкой с запятой и латинской «a» в классе.
    let csv = "Класс;Предмет;Учитель;Часов в неделю\n5a;Математика;Иванова А.С.;4\n6а;История;Новиков Пётр;2\n7а;Химия;Кто-то;полтора\n";
    let rows = table_import::read_table(csv.as_bytes(), "csv").unwrap();
    let r2 = table_import::import_loads(&mut s, &rows, false).unwrap();
    assert_eq!(r2.loads, 2);
    assert_eq!(r2.warnings.len(), 1);
    assert_eq!(s.classes.len(), 3, "5a = 5а, добавился только 6а");
}

#[test]
fn difficulty_table_roundtrip() {
    let mut s = demo::demo_school();
    let csv = table_import::difficulty_csv(&s);
    let mut rows = table_import::read_table(csv.as_bytes(), "csv").unwrap();
    rows[2][2] = "11".into();
    let name = rows[2][0].clone();
    let (n, w) = table_import::apply_difficulty(&mut s, &rows).unwrap();
    assert_eq!(n, s.subjects.len());
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(s.subjects.iter().find(|x| x.name == name).unwrap().difficulty[0], 11);
}

#[test]
fn control_schools_are_feasible_and_room_overload_is_explained() {
    for k in 1..=4 {
        let s = demo::scaled_school(k);
        assert_eq!(s.classes.len(), 13 * k);
        let errs: Vec<_> = precheck(&s).into_iter().filter(|i| i.severity == Severity::Error).collect();
        assert!(errs.is_empty(), "k={k}: {errs:#?}");
    }
    // Все начальные классы трёх копий в одном спортзале — не помещается, и precheck это объясняет.
    let mut s = demo::scaled_school(3);
    let gym = s.rooms.iter().find(|r| r.name == "Спортзал НШ").unwrap().id;
    for l in s.loads.iter_mut() {
        for g in l.groups.iter_mut() {
            if g.rooms.len() == 1 && s.rooms.iter().any(|r| r.id == g.rooms[0] && r.name.starts_with("Спортзал НШ")) {
                g.rooms = vec![gym];
            }
        }
    }
    let msgs: Vec<String> = precheck(&s).into_iter().filter(|i| i.code == "room_hours").map(|i| i.message).collect();
    assert!(msgs.iter().any(|m| m.contains("Спортзал НШ") && m.contains("Не встанут")), "{msgs:?}");
}

/// Регрессия 1.0: в режиме «считать N секунд» (так работает программа) генератор
/// не охлаждался и давал расписание в разы хуже, чем с ограничением итераций.
#[test]
fn time_limited_solve_is_as_good_as_iteration_limited() {
    let s = demo::demo_school();
    let by_iter = solver::solve(&s, &solver::SolveOptions { seed: 5, max_iterations: 400_000, time_limit: Duration::from_secs(60), ..Default::default() });
    let by_time = solver::solve(&s, &solver::SolveOptions { seed: 5, max_iterations: u64::MAX / 2, time_limit: Duration::from_secs(4), ..Default::default() });
    let a = analyze(&s, &by_iter.timetable).score;
    let b = analyze(&s, &by_time.timetable).score;
    assert_eq!(by_time.unplaced, 0);
    assert!(b <= a * 3 + 100, "по времени {b}, по итерациям {a}");
}
