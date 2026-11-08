// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
use chrono::NaiveDate;
use clap::{Parser, Subcommand};
use skyurok_core::export;
use skyurok_core::substitute::{auto_plan, check_replacements, SubContext, SubPolicy};
use skyurok_core::validate::{analyze, precheck, Severity};
use skyurok_core::*;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Parser)]
#[command(name = "skyurok", version, about = "SkyUrok — расписание и замены (консольная версия ядра)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Создать демо-школу, составить расписание, смоделировать отсутствия и замены, выгрузить всё.
    Demo {
        out: PathBuf,
        #[arg(long, default_value_t = 15)]
        seconds: u64,
        #[arg(long, default_value_t = 7)]
        seed: u64,
    },
    /// Составить (или улучшить с сохранением закреплённых) расписание.
    Solve {
        project: PathBuf,
        #[arg(short, long)]
        out: Option<PathBuf>,
        #[arg(long, default_value_t = 20)]
        seconds: u64,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Сколько вариантов считать параллельно (0 — по числу ядер, не больше 4).
        #[arg(long, default_value_t = 0)]
        threads: usize,
    },
    /// Страница замен для телефона (HTML) на сегодня и следующие учебные дни.
    Phone {
        project: PathBuf,
        out: PathBuf,
        /// С какой даты (ГГГГ-ММ-ДД), по умолчанию — сегодня.
        #[arg(long)]
        from: Option<NaiveDate>,
        #[arg(long, default_value_t = 2)]
        days: usize,
    },
    /// Замер генератора на контрольных школах: 13, 26, 39… классов.
    Bench {
        /// Размеры в копиях демо-школы (1 копия = 13 классов), через запятую.
        #[arg(long, default_value = "1,2,3,4")]
        sizes: String,
        #[arg(long, default_value_t = 20)]
        seconds: u64,
        #[arg(long, default_value_t = 0)]
        threads: usize,
        /// Вывод таблицей Markdown (для отчёта сборки на GitHub).
        #[arg(long)]
        markdown: bool,
    },
    /// Проверить исходные данные, расписание и замены.
    Check { project: PathBuf },
    /// Отметить отсутствие учителя.
    Absent {
        project: PathBuf,
        #[arg(long)]
        teacher: Id,
        #[arg(long)]
        from: NaiveDate,
        #[arg(long)]
        to: Option<NaiveDate>,
        /// Номера уроков с 1 через запятую (если отсутствует не весь день).
        #[arg(long, value_delimiter = ',')]
        periods: Option<Vec<u8>>,
        #[arg(long, default_value = "")]
        reason: String,
    },
    /// Показать затронутые уроки и варианты замены с объяснением.
    Suggest {
        project: PathBuf,
        #[arg(long)]
        date: NaiveDate,
        #[arg(long, default_value_t = 5)]
        top: usize,
    },
    /// Автоматически подобрать замены на период (черновики).
    Auto {
        project: PathBuf,
        #[arg(long)]
        from: NaiveDate,
        #[arg(long)]
        to: Option<NaiveDate>,
    },
    /// Выгрузить печатные формы и CSV.
    Export {
        project: PathBuf,
        out: PathBuf,
        #[arg(long)]
        from: Option<NaiveDate>,
        #[arg(long)]
        to: Option<NaiveDate>,
    },
    /// Выгрузить документ Word (.docx) или PDF (.pdf) — по расширению файла.
    Doc {
        project: PathBuf,
        out: PathBuf,
        /// classes, teachers, school, reps, tabel через запятую
        #[arg(long, value_delimiter = ',', default_value = "classes,reps")]
        what: Vec<String>,
        /// Пометка «Черновик» и лист проблем, если есть ошибки
        #[arg(long)]
        draft: bool,
        #[arg(long)]
        from: Option<NaiveDate>,
        #[arg(long)]
        to: Option<NaiveDate>,
    },
    /// Показать структуру файла .rtt.
    RttInfo { file: PathBuf },
    /// Перенести справочники из файла .rtt в новый проект.
    RttImport { file: PathBuf, out: PathBuf },
}

fn load(p: &Path) -> Project {
    let t = std::fs::read_to_string(p).unwrap_or_else(|e| die(&format!("{}: {e}", p.display())));
    Project::load_json(&t).unwrap_or_else(|e| die(&e))
}
fn save(p: &Path, pr: &Project) {
    std::fs::write(p, pr.to_json()).unwrap_or_else(|e| die(&format!("{}: {e}", p.display())));
}
fn die(m: &str) -> ! {
    eprintln!("Ошибка: {m}");
    std::process::exit(1)
}

fn print_report(pr: &Project) {
    let pre = precheck(&pr.school);
    if !pre.is_empty() {
        println!("Исходные данные:");
        for i in &pre {
            println!("  [{:?}] {}", i.severity, i.message);
        }
    }
    let r = analyze(&pr.school, &pr.timetable);
    println!("Расписание: {} уроков, ошибок {}, оценка {} (меньше — лучше)", pr.timetable.lessons.len(), r.errors(), r.score);
    for (k, v) in r.stats.lines() {
        println!("  {k}: {v}");
    }
    for i in r.issues.iter().filter(|i| i.severity == Severity::Error).take(15) {
        println!("  ✗ {}", i.message);
    }
    let ctx = SubContext::new(&pr.school, &pr.timetable, &pr.absences).with_classes(&pr.class_absences);
    let ri = check_replacements(&ctx, &pr.replacements);
    println!("Замены: {}, замечаний {}", pr.replacements.len(), ri.len());
    for i in ri.iter().take(15) {
        println!("  {} {}", if i.severity == Severity::Error { "✗" } else { "!" }, i.message);
    }
}

fn threads_or_default(t: usize) -> usize {
    if t == 0 {
        solver::default_threads()
    } else {
        t
    }
}

fn do_bench(sizes: &str, seconds: u64, threads: usize, markdown: bool) {
    let threads = threads_or_default(threads);
    if markdown {
        println!("### Замер генератора ({threads} потока, лимит {seconds} с)\n");
        println!("| Классов | Учителей | Уроков | Время, с | Конфликты | Не расставлено | Оценка |");
        println!("|---:|---:|---:|---:|---:|---:|---:|");
    } else {
        println!("Потоков: {threads}, лимит: {seconds} с");
        println!("{:>8} {:>9} {:>7} {:>9} {:>10} {:>14} {:>8}", "классов", "учителей", "уроков", "время, с", "конфликты", "не расставлено", "оценка");
    }
    for k in sizes.split(',').filter_map(|x| x.trim().parse::<usize>().ok()).filter(|k| *k > 0) {
        let school = demo::scaled_school(k);
        let opts = solver::SolveOptions { seed: 1, max_iterations: u64::MAX / 2, time_limit: Duration::from_secs(seconds), ..Default::default() };
        let res = solver::solve_parallel(&school, &opts, threads);
        let rep = analyze(&school, &res.timetable);
        let hard = rep.issues.iter().filter(|i| i.severity == Severity::Error && i.code != "unplaced").count();
        let hours: u32 = school.loads.iter().map(|l| l.hours_per_week as u32).sum();
        if markdown {
            println!(
                "| {} | {} | {} | {:.1} | {} | {} | {} |",
                school.classes.len(),
                school.teachers.len(),
                hours,
                res.elapsed.as_secs_f64(),
                hard,
                res.unplaced,
                res.score
            );
        } else {
            println!(
                "{:>8} {:>9} {:>7} {:>9.1} {:>10} {:>14} {:>8}",
                school.classes.len(),
                school.teachers.len(),
                hours,
                res.elapsed.as_secs_f64(),
                hard,
                res.unplaced,
                res.score
            );
        }
    }
}

fn do_solve(pr: &mut Project, seconds: u64, seed: u64, threads: usize) {
    let opts = solver::SolveOptions {
        seed,
        max_iterations: 3_000_000,
        time_limit: Duration::from_secs(seconds),
        keep_pinned_from: Some(pr.timetable.clone()),
        ..Default::default()
    };
    let res = solver::solve_parallel(&pr.school, &opts, threads_or_default(threads));
    println!(
        "Генерация: {} уроков, нерасставлено {}, оценка {}, итераций {}, {:.1} с",
        res.timetable.lessons.len(),
        res.unplaced,
        res.score,
        res.iterations,
        res.elapsed.as_secs_f64()
    );
    let today = chrono::Local::now().date_naive();
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
    let rep = history::apply_timetable(pr, res.timetable, today, &stamp, &format!("Перед составлением {stamp}"));
    if rep.touched() > 0 {
        println!("Замены: перенесено с пометкой «пересмотреть» {}, удалено {}", rep.shifted, rep.removed);
    }
}

fn do_export(pr: &Project, out: &Path, from: NaiveDate, to: NaiveDate) {
    std::fs::create_dir_all(out).unwrap();
    let w = |n: &str, s: String| std::fs::write(out.join(n), s).unwrap();
    w("расписание_классов.html", export::html_classes(pr, None));
    w("расписание_учителей.html", export::html_teachers(pr, None));
    w("общее_расписание.html", export::html_school(pr));
    w("замены.html", export::html_replacements(pr, from, to));
    w("расписание.csv", export::csv_timetable(pr));
    w("замены.csv", export::csv_replacements(pr, from, to));
    let mut d = from;
    while d <= to {
        if pr.replacements.iter().any(|r| r.date == d) {
            w(&format!("день_{}.html", d.format("%Y-%m-%d")), export::html_day(pr, d));
        }
        d = d.succ_opt().unwrap();
    }
    println!("Выгружено в {}", out.display());
}

fn range_of(pr: &Project) -> (NaiveDate, NaiveDate) {
    let ds: Vec<NaiveDate> = pr.replacements.iter().map(|r| r.date).chain(pr.absences.iter().flat_map(|a| [a.from, a.to])).collect();
    let today = chrono::Local::now().date_naive();
    (ds.iter().min().copied().unwrap_or(today), ds.iter().max().copied().unwrap_or(today))
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Demo { out, seconds, seed } => {
            std::fs::create_dir_all(&out).unwrap();
            let mut pr = Project::new(demo::demo_school());
            for i in precheck(&pr.school) {
                println!("[{:?}] {}", i.severity, i.message);
            }
            do_solve(&mut pr, seconds, seed, 0);
            // Отсутствия: математик на два дня, англичанка — первые три урока.
            let ix = Index::new(&pr.school);
            let math = pr
                .school
                .teachers
                .iter()
                .find(|t| {
                    t.subjects.contains(&4) && t.note.is_empty() && pr.school.loads.iter().any(|l| l.groups.iter().any(|g| g.teacher == t.id && g.subject == 5))
                })
                .map(|t| t.id)
                .unwrap();
            let eng = pr.school.teachers.iter().find(|t| t.subjects == vec![8]).map(|t| t.id).unwrap();
            let mon = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
            let (tue, wed) = (mon.succ_opt().unwrap(), mon.succ_opt().unwrap().succ_opt().unwrap());
            println!("Отсутствуют: {} (10–11.11, больничный), {} (10.11, 1–3 уроки)", ix.teacher_name(math), ix.teacher_name(eng));
            pr.absences.push(Absence { id: 1, teacher: math, from: tue, to: wed, reason: "больничный".into(), periods: None });
            pr.absences.push(Absence { id: 2, teacher: eng, from: tue, to: tue, reason: "курсы".into(), periods: Some(vec![0, 1, 2]) });
            let ctx = SubContext::new(&pr.school, &pr.timetable, &pr.absences).with_classes(&pr.class_absences);
            let mut next = 1;
            let plan = auto_plan(&ctx, &[], tue, wed, &SubPolicy::standard(), &mut next);
            println!("Автоподбор: {} замен, без решения {}", plan.replacements.len(), plan.unresolved.len());
            let reps = plan.replacements;
            drop(ctx);
            pr.replacements = reps;
            save(&out.join("проект.json"), &pr);
            print_report(&pr);
            do_export(&pr, &out.join("печать"), tue, wed);
        }
        Cmd::Solve { project, out, seconds, seed, threads } => {
            let mut pr = load(&project);
            do_solve(&mut pr, seconds, seed, threads);
            save(out.as_deref().unwrap_or(&project), &pr);
            print_report(&pr);
        }
        Cmd::Check { project } => print_report(&load(&project)),
        Cmd::Absent { project, teacher, from, to, periods, reason } => {
            let mut pr = load(&project);
            let id = pr.absences.iter().map(|a| a.id).max().unwrap_or(0) + 1;
            pr.absences.push(Absence {
                id,
                teacher,
                from,
                to: to.unwrap_or(from),
                reason,
                periods: periods.map(|v| v.into_iter().map(|p| p.saturating_sub(1)).collect()),
            });
            save(&project, &pr);
            println!("Отсутствие #{id} добавлено");
        }
        Cmd::Suggest { project, date, top } => {
            let pr = load(&project);
            let ctx = SubContext::new(&pr.school, &pr.timetable, &pr.absences).with_classes(&pr.class_absences);
            let reps: Vec<&Replacement> = pr.replacements.iter().collect();
            for a in ctx.affected(date, date) {
                let l = &pr.timetable.lessons[a.lesson_idx];
                let load = ctx.ix.load(l.load).unwrap();
                println!(
                    "\n{} урок, {} — {} ({})",
                    a.slot.period + 1,
                    ctx.ix.classes_label(&load.classes),
                    ctx.ix.subject_name(load.groups[a.group].subject),
                    ctx.ix.teacher_name(a.teacher)
                );
                for s in ctx.suggest(&a, &reps, &SubPolicy::standard()).iter().take(top) {
                    let who = match &s.kind {
                        ReplacementKind::Substitute { teacher, .. } => ctx.ix.teacher_name(*teacher),
                        ReplacementKind::Merge { .. } => "объединение".into(),
                        ReplacementKind::Cancel => "снять урок".into(),
                    };
                    println!(
                        "  {:>4}  {who}: {}{}",
                        s.score,
                        s.reasons.join(", "),
                        if s.warnings.is_empty() { String::new() } else { format!(" ⚠ {}", s.warnings.join(", ")) }
                    );
                }
            }
        }
        Cmd::Auto { project, from, to } => {
            let mut pr = load(&project);
            let to = to.unwrap_or(from);
            let ctx = SubContext::new(&pr.school, &pr.timetable, &pr.absences).with_classes(&pr.class_absences);
            let mut next = pr.replacements.iter().map(|r| r.id).max().unwrap_or(0) + 1;
            let plan = auto_plan(&ctx, &pr.replacements, from, to, &SubPolicy::standard(), &mut next);
            println!("Подобрано {} замен (черновики), без решения: {}", plan.replacements.len(), plan.unresolved.len());
            for (a, _) in &plan.unresolved {
                println!("  ? {} {} урок — нужен человек", a.date, a.slot.period + 1);
            }
            drop(ctx);
            pr.replacements.extend(plan.replacements);
            save(&project, &pr);
        }
        Cmd::Export { project, out, from, to } => {
            let pr = load(&project);
            let (f, t) = range_of(&pr);
            do_export(&pr, &out, from.unwrap_or(f), to.unwrap_or(t));
        }
        Cmd::Bench { sizes, seconds, threads, markdown } => do_bench(&sizes, seconds, threads, markdown),
        Cmd::Phone { project, out, from, days } => {
            let pr = load(&project);
            let from = from.unwrap_or_else(|| chrono::Local::now().date_naive());
            let stamp = chrono::Local::now().format("%d.%m.%Y %H:%M").to_string();
            let html = export::html_phone(&pr, from, days, &stamp);
            std::fs::write(&out, html).unwrap_or_else(|e| die(&format!("{}: {e}", out.display())));
            println!("Сохранено: {}", out.display());
        }
        Cmd::Doc { project, out, what, draft, from, to } => {
            use skyurok_core::printdoc::{build, problems, PrintOptions, Section};
            let pr = load(&project);
            let (f, t) = range_of(&pr);
            let mut o = PrintOptions::new(from.unwrap_or(f), to.unwrap_or(t));
            o.draft = draft;
            let secs: Vec<Section> = what
                .iter()
                .filter_map(|w| match w.as_str() {
                    "classes" => Some(Section::Classes),
                    "teachers" => Some(Section::Teachers),
                    "school" => Some(Section::School),
                    "reps" => Some(Section::Replacements),
                    "tabel" => Some(Section::Tabel),
                    _ => None,
                })
                .collect();
            let probs = problems(&pr);
            if !probs.is_empty() {
                println!(
                    "Внимание: {} {} в расписании{}",
                    probs.len(),
                    skyurok_core::printdoc::plural(probs.len(), "проблема", "проблемы", "проблем"),
                    if draft { " — документ помечен как черновик" } else { "" }
                );
            }
            let doc = build(&pr, &secs, &o);
            let bytes = match out.extension().and_then(|e| e.to_str()) {
                Some("pdf") => skyurok_core::export_pdf::write(&doc),
                _ => skyurok_core::export_docx::write(&doc),
            }
            .unwrap_or_else(|e| die(&e));
            std::fs::write(&out, bytes).unwrap_or_else(|e| die(&e.to_string()));
            println!("Сохранено: {} ({} стр. в модели)", out.display(), doc.pages.len());
        }
        Cmd::RttInfo { file } => {
            let b = std::fs::read(&file).unwrap_or_else(|e| die(&e.to_string()));
            let l = rtt::read_lists(&b).unwrap_or_else(|e| die(&e));
            println!("Секции:");
            for s in &l.sections {
                println!("  {:>8x}  {:<20} {:>8} байт", s.offset, s.name, s.size);
            }
            println!("Кабинетов: {}, предметов: {}, областей: {}, классов: {}", l.rooms.len(), l.subjects.len(), l.areas.len(), l.classes.len());
            for w in &l.warnings {
                println!("  ! {w}");
            }
        }
        Cmd::RttImport { file, out } => {
            let b = std::fs::read(&file).unwrap_or_else(|e| die(&e.to_string()));
            let l = rtt::read_lists(&b).unwrap_or_else(|e| die(&e));
            let name = file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let pr = Project::new(rtt::to_school(&l, &name));
            save(&out, &pr);
            println!("Перенесено: кабинетов {}, предметов {}, классов {}", pr.school.rooms.len(), pr.school.subjects.len(), pr.school.classes.len());
            for w in &l.warnings {
                println!("  ! {w}");
            }
        }
    }
}
