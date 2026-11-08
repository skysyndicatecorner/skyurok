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
