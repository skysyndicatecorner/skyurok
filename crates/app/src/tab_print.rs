// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::app::{SkyApp, Tab};
use crate::{fileio, theme, widgets};
use chrono::{Datelike, NaiveDate};
use eframe::egui::{self, RichText};
use skyurok_core::export;
use skyurok_core::printdoc::{self, PrintOptions, Section};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Word,
    Pdf,
    Html,
    Csv,
    Json,
}

impl Format {
    fn label(self) -> &'static str {
        match self {
            Format::Word => "Word",
            Format::Pdf => "PDF",
            Format::Html => "HTML",
            Format::Csv => "CSV (Excel)",
            Format::Json => "JSON",
        }
    }
    fn note(self) -> &'static str {
        match self {
            Format::Word => ".docx · можно править",
            Format::Pdf => "для печати и рассылки",
            Format::Html => "открыть в браузере",
            Format::Csv => "таблица для Excel",
            Format::Json => "сайт, SchoolTouch, бот",
        }
    }
    fn ext(self) -> &'static str {
        match self {
            Format::Word => "docx",
            Format::Pdf => "pdf",
            Format::Html => "html",
            Format::Csv => "csv",
            Format::Json => "json",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DraftChoice {
    Draft,
    AsIs,
    Fix,
}

pub struct State {
    format: Format,
    classes: bool,
    teachers: bool,
    school: bool,
    reps: bool,
    tabel: bool,
    day: bool,
    approve: bool,
    bells: bool,
    grayscale: bool,
    full_names: bool,
    rooms: bool,
    reasons: bool,
    from: NaiveDate,
    to: NaiveDate,
    from_buf: String,
    to_buf: String,
    /// Окно предупреждения: список проблем и выбор.
    warn: Option<(Vec<String>, DraftChoice, bool)>,
    last_file: Option<std::path::PathBuf>,
}

impl Default for State {
    fn default() -> Self {
        let t = chrono::Local::now().date_naive();
        let mon = t - chrono::Duration::days(t.weekday().num_days_from_monday() as i64);
        Self {
            format: Format::Word,
            classes: true,
            teachers: false,
            school: false,
            reps: false,
            tabel: false,
            day: false,
            approve: true,
            bells: true,
            grayscale: false,
            full_names: false,
            rooms: true,
            reasons: false,
            from: mon,
            to: mon + chrono::Duration::days(6),
            from_buf: String::new(),
            to_buf: String::new(),
            warn: None,
            last_file: None,
        }
    }
}

impl State {
    /// Настроить на «лист замен за период» (из раздела «Замены»).
    pub fn preset_replacements(&mut self, from: NaiveDate, to: NaiveDate, f: Format) {
        self.format = f;
        self.classes = false;
        self.teachers = false;
        self.school = false;
        self.tabel = false;
        self.reps = true;
        self.from = from;
        self.to = to;
        self.from_buf.clear();
        self.to_buf.clear();
    }
}

fn sections(st: &State) -> Vec<Section> {
    let mut v = vec![];
    if st.classes {
        v.push(Section::Classes);
    }
    if st.teachers {
        v.push(Section::Teachers);
    }
    if st.school {
        v.push(Section::School);
    }
    if st.reps {
        v.push(Section::Replacements);
    }
    if st.tabel {
        v.push(Section::Tabel);
    }
    v
}

/// Начать выгрузку: при проблемах сначала спросить.
pub fn start_export(app: &mut SkyApp, f: Format) {
    app.print_ui.format = f;
    let st = &app.print_ui;
    if sections(st).is_empty() && !(f == Format::Html && st.day) && f != Format::Json && f != Format::Csv {
        app.notify("Выберите, что выгрузить", true);
        return;
    }
    let probs = printdoc::problems(&app.project);
    if !probs.is_empty() && matches!(f, Format::Word | Format::Pdf | Format::Html) {
        app.print_ui.warn = Some((probs, DraftChoice::Draft, false));
    } else {
        run_export(app, false, false);
    }
}

pub fn warning_dialog(app: &mut SkyApp, ctx: &egui::Context) {
    let Some((probs, mut choice, print_only)) = app.print_ui.warn.clone() else { return };
    let mut go = false;
    let mut cancel = false;
    egui::Modal::new(egui::Id::unique("export_warn")).show(ctx, |ui| {
        ui.set_width(560.0);
        egui::Frame::new().fill(theme::warn_bg()).corner_radius(8.0).inner_margin(12.0).stroke(egui::Stroke::new(2.0, theme::warn())).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                theme::bold(format!(
                    "! В расписании {} {} — выгрузить можно, но проверьте",
                    probs.len(),
                    printdoc::plural(probs.len(), "проблема", "проблемы", "проблем")
                ))
                .color(theme::warn()),
            );
            for p in probs.iter().take(8) {
                ui.label(RichText::new(format!("•  {p}")).color(theme::text()));
            }
            if probs.len() > 8 {
                ui.label(theme::small(format!("и ещё {}", probs.len() - 8)));
            }
        });
        ui.add_space(8.0);
        ui.label(theme::bold("Как поступить"));
        ui.radio_value(&mut choice, DraftChoice::Draft, "Выгрузить с пометкой «Черновик» — водяной знак и лист проблем в конце");
        ui.radio_value(&mut choice, DraftChoice::AsIs, "Выгрузить как есть — без пометок");
        ui.radio_value(&mut choice, DraftChoice::Fix, "Сначала исправить — перейти к расписанию");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(theme::bold("Продолжить").color(theme::accent_text())).fill(theme::accent())).clicked() {
                go = true;
            }
            if ui.button("Отмена").clicked() {
                cancel = true;
            }
        });
    });
    if go {
        app.print_ui.warn = None;
        match choice {
            DraftChoice::Fix => {
                app.tab = Tab::Timetable;
            }
            DraftChoice::Draft => run_export(app, true, print_only),
            DraftChoice::AsIs => run_export(app, false, print_only),
        }
    } else if cancel {
        app.print_ui.warn = None;
    } else if let Some(w) = app.print_ui.warn.as_mut() {
        w.1 = choice;
    }
}

fn options(app: &SkyApp, draft: bool) -> PrintOptions {
    let st = &app.print_ui;
    let mut o = PrintOptions::new(st.from, st.to);
    o.approve = st.approve;
    o.bells = st.bells;
    o.grayscale = st.grayscale;
    o.full_names = st.full_names;
    o.rooms = st.rooms;
    o.reasons = st.reasons;
    o.draft = draft;
    o
}

fn default_name(app: &SkyApp, ext: &str) -> String {
    let st = &app.print_ui;
    let what = if st.reps && !st.classes && !st.teachers && !st.school { "Замены" } else { "Расписание" };
    format!("{what} {}.{ext}", st.from.format("%Y-%m-%d"))
}

/// Выгрузка. `to_print` — сохранить во временную папку и сразу открыть.
fn run_export(app: &mut SkyApp, draft: bool, to_print: bool) {
    let f = app.print_ui.format;
    let p = app.project.clone();
    match f {
        Format::Word | Format::Pdf => {
            let doc = printdoc::build(&p, &sections(&app.print_ui), &options(app, draft));
            if doc.pages.is_empty() {
                app.notify("Нечего выгружать: в выбранном нет данных (например, нет замен за период)", true);
                return;
            }
            let bytes = if f == Format::Word { skyurok_core::export_docx::write(&doc) } else { skyurok_core::export_pdf::write(&doc) };
            let bytes = match bytes {
                Ok(b) => b,
                Err(e) => {
                    app.notify(format!("Ошибка выгрузки: {e}"), true);
                    return;
                }
            };
            let name = default_name(app, f.ext());
            let path = if to_print { Some(fileio::data_dir().join("print").join(&name)) } else { fileio::pick_save(f.label(), &[f.ext()], &name) };
            let Some(path) = path else { return };
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::write(&path, bytes) {
                Ok(()) => {
                    app.print_ui.last_file = Some(path.clone());
                    if to_print {
                        let _ = fileio::open_external(&path);
                    }
                    app.notify(format!("Готово{}: {}", if doc.draft.is_some() { " (черновик)" } else { "" }, path.display()), false);
                }
                Err(e) => app.notify(format!("Не удалось сохранить: {e}"), true),
            }
        }
        Format::Html => {
            let st = &app.print_ui;
            let mut files: Vec<(String, String)> = vec![];
            if st.classes {
                files.push(("расписание_классов.html".into(), export::html_classes(&p, None)));
            }
            if st.teachers {
                files.push(("расписание_учителей.html".into(), export::html_teachers(&p, None)));
            }
            if st.school {
                files.push(("общее_расписание.html".into(), export::html_school(&p)));
            }
            if st.reps {
                files.push((format!("замены_{}_{}.html", st.from, st.to), export::html_replacements(&p, st.from, st.to)));
            }
            if st.day {
                let mut d = st.from;
                while d <= st.to {
                    if skyurok_core::model::day_of(d, p.school.settings.days_per_week).is_some() {
                        files.push((format!("день_{d}.html"), export::html_day(&p, d)));
                    }
                    d = d.succ_opt().unwrap();
                }
            }
            if draft {
                for (_, h) in files.iter_mut() {
                    *h = h.replacen("<body>", "<body><div style=\"border:2px solid #B42318;color:#B42318;padding:8px;margin-bottom:12px;font-weight:600\">ЧЕРНОВИК: в расписании есть нерешённые проблемы</div>", 1);
                }
            }
            let dir = fileio::data_dir().join("print");
            for (n, h) in &files {
                let _ = fileio::write(&dir.join(n), h);
            }
            for (n, _) in files.iter().take(6) {
                let _ = fileio::open_external(&dir.join(n));
            }
            app.notify(format!("Открыто в браузере: {} файл(ов)", files.len()), false);
        }
        Format::Csv | Format::Json => {
            let st = &app.print_ui;
            let (text, name) = if f == Format::Csv {
                let mut t = export::csv_timetable(&p);
                if st.reps {
                    t = export::csv_replacements(&p, st.from, st.to);
                }
                (t, default_name(app, "csv"))
            } else {
                (export::json_public(&p, st.from, st.to), default_name(app, "json"))
            };
            if let Some(path) = fileio::pick_save(f.label(), &[f.ext()], &name) {
                match fileio::write(&path, &text) {
                    Ok(()) => app.notify(format!("Сохранено: {}", path.display()), false),
                    Err(e) => app.notify(e, true),
                }
            }
        }
    }
}

pub fn show(app: &mut SkyApp, ui: &mut egui::Ui) {
    widgets::heading(ui, "Печать и экспорт");
    ui.label(theme::small(
        "Word и PDF создаются прямо в программе, Office не нужен. Если в расписании есть проблемы, программа предупредит и предложит пометку «Черновик».",
    ));
    ui.add_space(6.0);
    let mut do_export = false;
    let mut do_print = false;
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.columns(2, |cols| {
            let ui = &mut cols[0];
            widgets::card(ui, |ui| {
                let st = &mut app.print_ui;
                ui.label(theme::bold("Формат"));
                ui.horizontal_wrapped(|ui| {
                    for f in [Format::Word, Format::Pdf, Format::Html, Format::Csv, Format::Json] {
                        let r = ui.add_sized([128.0, 48.0], egui::Button::new(format!("{}\n{}", f.label(), f.note())).selected(st.format == f));
                        if r.clicked() {
                            st.format = f;
                        }
                    }
                });
                ui.add_space(8.0);
                ui.label(theme::bold("Что выгрузить"));
                ui.checkbox(&mut st.classes, "Расписание классов — класс на страницу");
                ui.checkbox(&mut st.teachers, "Расписание учителей — учитель на страницу");
                ui.checkbox(&mut st.school, "Общее расписание — день на страницу (альбомная)");
                ui.checkbox(&mut st.reps, "Лист замен — по дням периода");
                ui.checkbox(&mut st.tabel, "Табель замен за период — для бухгалтерии");
                if st.format == Format::Html {
                    ui.checkbox(&mut st.day, "Расписание каждого дня с заменами — для стенда");
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Период замен: с");
                    widgets::date_field(ui, &mut st.from, &mut st.from_buf);
                    ui.label("по");
                    widgets::date_field(ui, &mut st.to, &mut st.to_buf);
                });
                if st.to < st.from {
                    st.to = st.from;
                    st.to_buf.clear();
                }
            });
            let ui = &mut cols[1];
            widgets::card(ui, |ui| {
                let st = &mut app.print_ui;
                ui.label(theme::bold("Оформление (Word и PDF)"));
                ui.checkbox(&mut st.approve, "Гриф «Утверждаю» с подписью директора");
                ui.checkbox(&mut st.bells, "Время звонков в таблице");
                ui.checkbox(&mut st.rooms, "Кабинеты");
                ui.checkbox(&mut st.reasons, "Причины отсутствия на листе замен")
                    .on_hover_text("Лист висит на виду, а «больничный» — личные сведения. По умолчанию причины не печатаются.");
                ui.checkbox(&mut st.full_names, "Имена и отчества полностью");
                ui.checkbox(&mut st.grayscale, "Чёрно-белая печать (без заливок)");
                ui.label(theme::small("Проблемные уроки отмечены рамкой и знаком «!» — видно и в ч/б."));
            });
            ui.add_space(8.0);
            widgets::card(ui, |ui| {
                let probs = printdoc::problems(&app.project);
                if probs.is_empty() {
                    ui.label(RichText::new("✓ Проблем нет — документ будет чистовым").color(theme::ok()));
                } else {
                    ui.label(
                        RichText::new(format!(
                            "! {} {} — при выгрузке программа предложит «Черновик»",
                            probs.len(),
                            printdoc::plural(probs.len(), "проблема", "проблемы", "проблем")
                        ))
                        .color(theme::warn()),
                    );
                }
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    let f = app.print_ui.format;
                    if ui.add(egui::Button::new(theme::bold(format!("Сохранить в {}…", f.label())).color(theme::accent_text())).fill(theme::accent())).clicked()
                    {
                        do_export = true;
                    }
                    if ui.button("Открыть для печати (PDF)").clicked() {
                        do_print = true;
                    }
                });
                if let Some(f) = app.print_ui.last_file.clone() {
                    ui.horizontal(|ui| {
                        ui.label(theme::small(format!("Последний файл: {}", f.display())));
                        if ui.small_button("Открыть").clicked() {
                            let _ = fileio::open_external(&f);
                        }
                    });
                }
            });
        });
    });
    if do_export {
        let f = app.print_ui.format;
        start_export(app, f);
    }
    if do_print {
        let probs = printdoc::problems(&app.project);
        app.print_ui.format = Format::Pdf;
        if probs.is_empty() {
            run_export(app, false, true);
        } else {
            app.print_ui.warn = Some((probs, DraftChoice::Draft, true));
        }
    }
}
