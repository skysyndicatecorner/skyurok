// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::app::SkyApp;
use crate::{fileio, tab_print, theme, widgets};
use chrono::{Datelike, NaiveDate};
use eframe::egui::{self, RichText};
use skyurok_core::export;
use skyurok_core::model::*;
use skyurok_core::substitute::{auto_plan, check_replacements, stats, AffectedLesson, SubContext, SubPolicy};
use skyurok_core::validate::Severity;
use std::collections::HashSet;

pub struct State {
    date: NaiveDate,
    date_buf: String,
    selected: Option<(Id, usize)>,
    f_teacher: Option<Id>,
    f_from: NaiveDate,
    f_to: NaiveDate,
    f_from_buf: String,
    f_to_buf: String,
    f_whole: bool,
    f_periods: Vec<bool>,
    f_reason: String,
    allow_cancel: bool,
    allow_day_off: bool,
    /// Форма отсутствия: false — учитель, true — класс.
    f_is_class: bool,
    f_class: Option<Id>,
}

impl Default for State {
    fn default() -> Self {
        let today = chrono::Local::now().date_naive();
        Self {
            date: today,
            date_buf: String::new(),
            selected: None,
            f_teacher: None,
            f_from: today,
            f_to: today,
            f_from_buf: String::new(),
            f_to_buf: String::new(),
            f_whole: true,
            f_periods: vec![false; 16],
            f_reason: String::new(),
            allow_cancel: true,
            allow_day_off: true,
            f_is_class: false,
            f_class: None,
        }
    }
}

impl State {
    pub fn set_date(&mut self, d: NaiveDate) {
        self.date = d;
        self.date_buf.clear();
        self.f_from = d;
        self.f_to = d;
    }
}

fn week_of(d: NaiveDate) -> (NaiveDate, NaiveDate) {
    let mon = d - chrono::Duration::days(d.weekday().num_days_from_monday() as i64);
    (mon, mon + chrono::Duration::days(6))
}

fn kind_text(ix: &Index, r: &Replacement, ctx: &SubContext) -> String {
    match &r.kind {
        ReplacementKind::Substitute { teacher, subject } => {
            format!("{} ({})", ix.teacher_name(*teacher), ix.subject(*subject).map(|s| s.short.clone()).unwrap_or_default())
        }
        ReplacementKind::Merge { with_lesson, group } => ctx
            .lesson(*with_lesson)
            .and_then(|l| ix.load(l.load))
            .map(|ld| format!("объединить: {} ({})", ix.teacher_name(ld.groups[(*group).min(ld.groups.len() - 1)].teacher), ix.classes_label(&ld.classes)))
            .unwrap_or_else(|| "объединение".into()),
        ReplacementKind::Cancel => "урок снят".into(),
    }
}

pub fn show(app: &mut SkyApp, ui: &mut egui::Ui) {
    let today = chrono::Local::now().date_naive();
    let date = app.subs_ui.date;
    ui.horizontal_wrapped(|ui| {
        widgets::heading(ui, "Замены");
        ui.add_space(10.0);
        let mut d = app.subs_ui.date;
        if widgets::date_field(ui, &mut d, &mut app.subs_ui.date_buf) {
            app.subs_ui.date = d;
            app.subs_ui.selected = None;
        }
        if ui.small_button("Сегодня").clicked() {
            app.subs_ui.date = today;
            app.subs_ui.date_buf.clear();
        }
        let wd = day_of(date, 7).map(|x| DAY_NAMES_FULL[x as usize]).unwrap_or("");
        ui.label(theme::bold(wd));
        ui.separator();
        let policy = SubPolicy { min_auto_score: 20, allow_cancel_edge: app.subs_ui.allow_cancel, allow_day_off_teachers: app.subs_ui.allow_day_off };
        if ui.button(theme::bold("Подобрать замены на день")).clicked() {
            auto(app, date, date, &policy);
        }
        if ui.button("Подобрать на неделю").clicked() {
            let (a, b) = week_of(date);
            auto(app, a, b, &policy);
        }
        if ui.button("Подтвердить черновики дня").clicked() {
            app.edit(|p| p.replacements.iter_mut().filter(|r| r.date == date).for_each(|r| r.confirmed = true));
        }
        ui.menu_button("Печать…", |ui| {
            if ui.button("Лист замен на день — Word").clicked() {
                print_doc(app, date, date, tab_print::Format::Word);
                ui.close();
            }
            if ui.button("Лист замен на день — PDF").clicked() {
                print_doc(app, date, date, tab_print::Format::Pdf);
                ui.close();
            }
            if ui.button("Лист замен на неделю — Word").clicked() {
                let (a, b) = week_of(date);
                print_doc(app, a, b, tab_print::Format::Word);
                ui.close();
            }
            if ui.button("Лист замен на неделю — PDF").clicked() {
                let (a, b) = week_of(date);
                print_doc(app, a, b, tab_print::Format::Pdf);
                ui.close();
            }
            ui.separator();
            if ui.button("Лист замен на этот день (браузер)").clicked() {
                print(app, date, date);
                ui.close();
            }
            if ui.button("Лист замен на неделю (браузер)").clicked() {
                let (a, b) = week_of(date);
                print(app, a, b);
                ui.close();
            }
            if ui.button("Страница замен для телефона…").on_hover_text("Один HTML-файл на сегодня и следующий учебный день. Выложите его на сайт школы — адрес укажите в разделе «Школа», и на листе замен появится QR-код.").clicked() {
                let stamp = chrono::Local::now().format("%d.%m.%Y %H:%M").to_string();
                let html = export::html_phone(&app.project, date, 2, &stamp);
                if let Some(path) = fileio::pick_save("Страница HTML", &["html"], "zameny.html") {
                    match fileio::write(&path, &html) {
                        Ok(()) => {
                            let _ = fileio::open_external(&path);
                            let hint = if app.project.school.settings.public_url.trim().is_empty() {
                                " Укажите её адрес на сайте в разделе «Школа» — на листе замен появится QR-код."
                            } else {
                                ""
                            };
                            app.notify(format!("Страница сохранена: {}.{hint}", path.display()), false);
                        }
                        Err(e) => app.notify(e, true),
                    }
                }
                ui.close();
            }
            if ui.button("Расписание дня с заменами (для стенда)").clicked() {
                let html = export::html_day(&app.project, date);
                open_html(app, &format!("день_{}.html", date), &html);
                ui.close();
            }
        });
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut app.subs_ui.allow_cancel, "можно снимать крайние уроки");
        ui.checkbox(&mut app.subs_ui.allow_day_off, "можно звать учителей без уроков в этот день");
    });
    ui.separator();

    egui::Panel::left("subs_left").default_size(270.0).resizable(true).show(ui, |ui| left(app, ui, today));
    egui::Panel::right("subs_right").default_size(350.0).resizable(true).show(ui, |ui| suggestions(app, ui));
    egui::CentralPanel::default().show(ui, |ui| center(app, ui));
}

/// Подобрать замены на выбранный в разделе день (для Ctrl+K).
pub fn auto_today(app: &mut SkyApp) {
    let date = app.subs_ui.date;
    let policy = SubPolicy { min_auto_score: 20, allow_cancel_edge: app.subs_ui.allow_cancel, allow_day_off_teachers: app.subs_ui.allow_day_off };
    auto(app, date, date, &policy);
}

fn print_doc(app: &mut SkyApp, from: NaiveDate, to: NaiveDate, f: tab_print::Format) {
    app.print_ui.preset_replacements(from, to, f);
    tab_print::start_export(app, f);
}

fn auto(app: &mut SkyApp, from: NaiveDate, to: NaiveDate, policy: &SubPolicy) {
    let p = app.project.clone();
    let ctx = SubContext::new(&p.school, &p.timetable, &p.absences).with_classes(&p.class_absences);
    let mut next = widgets::next_id(&p.replacements, |r| r.id);
    let plan = auto_plan(&ctx, &p.replacements, from, to, policy, &mut next);
    let n = plan.replacements.len();
    let u = plan.unresolved.len();
    if n == 0 && u == 0 {
        app.notify("Нечего подбирать: нет уроков отсутствующих без замены", false);
        return;
    }
    app.edit(|pp| pp.replacements.extend(plan.replacements));
    app.notify(format!("Подобрано замен: {n} (черновики){}", if u > 0 { format!(", без решения: {u} — выберите вручную") } else { String::new() }), u > 0);
}

fn open_html(app: &mut SkyApp, name: &str, html: &str) {
    let path = fileio::data_dir().join("print").join(name);
    match fileio::write(&path, html).and_then(|_| fileio::open_external(&path)) {
        Ok(()) => app.notify(format!("Открыто для печати: {}", path.display()), false),
        Err(e) => app.notify(format!("Файл: {} ({e})", path.display()), true),
    }
}

fn print(app: &mut SkyApp, from: NaiveDate, to: NaiveDate) {
    let html = export::html_replacements(&app.project, from, to);
    open_html(app, &format!("замены_{}_{}.html", from, to), &html);
}

fn left(app: &mut SkyApp, ui: &mut egui::Ui, today: NaiveDate) {
    let s = app.project.school.clone();
    let ix = Index::new(&s);
    egui::ScrollArea::vertical().id_salt("subs_l").show(ui, |ui| {
        let mut marked = HashSet::new();
        for (from, to) in app.project.absences.iter().map(|a| (a.from, a.to)).chain(app.project.class_absences.iter().map(|a| (a.from, a.to))) {
            let (af, at) = (from, to);
            let mut d = af;
            while d <= at && d - af < chrono::Duration::days(400) {
                marked.insert(d);
                d = d.succ_opt().unwrap();
            }
        }
        let mut d = app.subs_ui.date;
        if widgets::mini_calendar(ui, &mut d, &marked, today) {
            app.subs_ui.date = d;
            app.subs_ui.date_buf.clear();
            app.subs_ui.selected = None;
        }
        ui.label(theme::small("Красным — дни, когда кто-то отсутствует."));
        ui.separator();
        let date = app.subs_ui.date;
        ui.label(theme::bold("Отсутствуют в этот день"));
        let list: Vec<Absence> = app.project.absences.iter().filter(|a| date >= a.from && date <= a.to).cloned().collect();
        if list.is_empty() {
            ui.label(theme::small("Никого"));
        }
        for a in list {
            ui.horizontal(|ui| {
                let per =
                    a.periods.as_ref().map(|p| format!(" ({} ур.)", p.iter().map(|x| (x + 1).to_string()).collect::<Vec<_>>().join(","))).unwrap_or_default();
                ui.label(format!("{}{}", ix.teacher_name(a.teacher), per));
                if a.from != a.to {
                    ui.label(theme::small(format!("{}–{}", a.from.format("%d.%m"), a.to.format("%d.%m"))));
                }
                if ui.small_button("✕").on_hover_text("Удалить отсутствие (и его замены)").clicked() {
                    let id = a.id;
                    let t = a.teacher;
                    let (f, to) = (a.from, a.to);
                    app.edit(|p| {
                        p.absences.retain(|x| x.id != id);
                        p.replacements.retain(|r| !(r.original_teacher == t && r.date >= f && r.date <= to));
                    });
                }
            });
            if !a.reason.is_empty() {
                ui.label(theme::small(format!("   {}", a.reason)));
            }
        }
        let clist: Vec<ClassAbsence> = app.project.class_absences.iter().filter(|a| date >= a.from && date <= a.to).cloned().collect();
        if !clist.is_empty() {
            ui.add_space(4.0);
            ui.label(theme::bold("Классы отсутствуют"));
            ui.label(theme::small("Экскурсия, карантин… Их уроки не требуют замены, учителя свободны."));
        }
        for a in clist {
            ui.horizontal(|ui| {
                let per =
                    a.periods.as_ref().map(|p| format!(" ({} ур.)", p.iter().map(|x| (x + 1).to_string()).collect::<Vec<_>>().join(","))).unwrap_or_default();
                ui.label(format!("{} класс{}", ix.class_name(a.class), per));
                if a.from != a.to {
                    ui.label(theme::small(format!("{}–{}", a.from.format("%d.%m"), a.to.format("%d.%m"))));
                }
                if ui.small_button("✕").on_hover_text("Удалить отсутствие класса").clicked() {
                    let id = a.id;
                    app.edit(|p| p.class_absences.retain(|x| x.id != id));
                }
            });
            if !a.reason.is_empty() {
                ui.label(theme::small(format!("   {}", a.reason)));
            }
        }
        ui.separator();
        ui.label(theme::bold("Отметить отсутствие"));
        let teachers: Vec<(Id, String)> = s.teachers.iter().map(|t| (t.id, t.short_name())).collect();
        let classes: Vec<(Id, String)> = s.classes.iter().map(|c| (c.id, c.name.clone())).collect();
        let st = &mut app.subs_ui;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut st.f_is_class, false, "Учитель");
            ui.selectable_value(&mut st.f_is_class, true, "Класс");
        });
        if st.f_is_class {
            widgets::combo_opt(ui, "abs_c", &mut st.f_class, &classes, "— класс —");
        } else {
            widgets::combo_opt(ui, "abs_t", &mut st.f_teacher, &teachers, "— учитель —");
        }
        if st.f_from < date && st.f_to < date {
            st.f_from = date;
            st.f_to = date;
            st.f_from_buf.clear();
            st.f_to_buf.clear();
        }
        ui.horizontal(|ui| {
            ui.label("с");
            widgets::date_field(ui, &mut st.f_from, &mut st.f_from_buf);
        });
        ui.horizontal(|ui| {
            ui.label("по");
            widgets::date_field(ui, &mut st.f_to, &mut st.f_to_buf);
        });
        if st.f_to < st.f_from {
            st.f_to = st.f_from;
            st.f_to_buf.clear();
        }
        ui.checkbox(&mut st.f_whole, "весь день");
        if !st.f_whole {
            ui.horizontal_wrapped(|ui| {
                for p in 0..s.settings.periods_per_day as usize {
                    ui.checkbox(&mut st.f_periods[p], format!("{}", p + 1));
                }
            });
        }
        ui.add(egui::TextEdit::singleline(&mut st.f_reason).hint_text(if st.f_is_class {
            "причина: экскурсия, карантин…"
        } else {
            "причина: больничный, курсы…"
        }));
        let who_ok = if st.f_is_class { st.f_class.is_some() } else { st.f_teacher.is_some() };
        let ok = who_ok && (st.f_whole || st.f_periods.iter().any(|x| *x));
        if st.f_is_class && ui.add_enabled(ok, egui::Button::new(theme::bold("Добавить отсутствие класса"))).clicked() {
            let a = ClassAbsence {
                id: widgets::next_id(&app.project.class_absences, |a| a.id),
                class: st.f_class.unwrap(),
                from: st.f_from,
                to: st.f_to,
                reason: st.f_reason.clone(),
                periods: if st.f_whole { None } else { Some(st.f_periods.iter().enumerate().filter(|(_, x)| **x).map(|(i, _)| i as u8).collect()) },
            };
            st.f_reason.clear();
            let name = ix.class_name(a.class);
            let (f, t) = (a.from, a.to);
            let cid = a.class;
            app.edit(|p| {
                p.class_absences.push(a);
                // замены уроков этого класса больше не нужны
                let lessons: HashSet<Id> =
                    p.timetable.lessons.iter().filter(|l| p.school.loads.iter().any(|ld| ld.id == l.load && ld.classes.contains(&cid))).map(|l| l.id).collect();
                p.replacements.retain(|r| !(r.date >= f && r.date <= t && lessons.contains(&r.lesson) && !r.confirmed));
            });
            app.notify(format!("{name}: отсутствие класса добавлено — его учителя свободны для замен"), false);
        } else if !st.f_is_class && ui.add_enabled(ok, egui::Button::new(theme::bold("Добавить отсутствие"))).clicked() {
            let a = Absence {
                id: widgets::next_id(&app.project.absences, |a| a.id),
                teacher: st.f_teacher.unwrap(),
                from: st.f_from,
                to: st.f_to,
                reason: st.f_reason.clone(),
                periods: if st.f_whole { None } else { Some(st.f_periods.iter().enumerate().filter(|(_, x)| **x).map(|(i, _)| i as u8).collect()) },
            };
            st.f_reason.clear();
            let name = ix.teacher_name(a.teacher);
            app.edit(|p| p.absences.push(a));
            app.notify(format!("{name}: отсутствие добавлено. Нажмите «Подобрать на день» или выберите замены вручную."), false);
        }
    });
}

fn center(app: &mut SkyApp, ui: &mut egui::Ui) {
    let p = app.project.clone();
    let date = app.subs_ui.date;
    let ix = Index::new(&p.school);
    let ctx = SubContext::new(&p.school, &p.timetable, &p.absences).with_classes(&p.class_absences);
    let affected = ctx.affected(date, date);
    let mut select = None;
    let mut remove = None;
    let mut confirm = None;
    egui::ScrollArea::both().id_salt("subs_c").show(ui, |ui| {
        ui.label(RichText::new(format!("Уроки, которые нужно закрыть: {}", affected.len())).family(crate::theme::bold_family()));
        if affected.is_empty() {
            ui.label(theme::small(if day_of(date, p.school.settings.days_per_week).is_none() {
                "Выходной день."
            } else {
                "На этот день отсутствующих нет. Отметьте отсутствие слева."
            }));
        }
        egui::Grid::new("aff").striped(true).num_columns(6).spacing([12.0, 6.0]).show(ui, |ui| {
            if !affected.is_empty() {
                for h in ["Урок", "Класс", "Предмет", "Отсутствует", "Замена", ""] {
                    ui.label(theme::bold(h));
                }
                ui.end_row();
            }
            for a in &affected {
                let l = &p.timetable.lessons[a.lesson_idx];
                let Some(load) = ix.load(l.load) else { continue };
                let g = &load.groups[a.group];
                let rep = p.replacements.iter().find(|r| r.date == date && r.lesson == a.lesson_id && r.group == a.group);
                let sel = app.subs_ui.selected == Some((a.lesson_id, a.group));
                if ui.selectable_label(sel, theme::bold(format!("{}", a.slot.period + 1))).clicked() {
                    select = Some((a.lesson_id, a.group));
                }
                if ui
                    .selectable_label(
                        sel,
                        format!("{}{}", ix.classes_label(&load.classes), if g.label.is_empty() { String::new() } else { format!(" {}", g.label) }),
                    )
                    .clicked()
                {
                    select = Some((a.lesson_id, a.group));
                }
                ui.label(ix.subject_name(g.subject));
                ui.label(ix.teacher_name(a.teacher));
                match rep {
                    Some(r) => {
                        let color = if r.kind == ReplacementKind::Cancel {
                            theme::bad()
                        } else if r.confirmed {
                            theme::ok()
                        } else {
                            theme::warn()
                        };
                        let color = if r.stale { theme::warn() } else { color };
                        let tag = if r.confirmed { "" } else { " · черновик" };
                        let text = if r.stale {
                            format!("! пересмотреть: {}", kind_text(&ix, r, &ctx))
                        } else {
                            format!("{}{tag}", kind_text(&ix, r, &ctx))
                        };
                        ui.label(RichText::new(text).color(color)).on_hover_text(if r.stale {
                            "После пересоставления урок переехал. Проверьте, свободен ли заменяющий, и нажмите ✓"
                        } else {
                            ""
                        });
                        ui.horizontal(|ui| {
                            if (!r.confirmed || r.stale) && ui.small_button("✓").on_hover_text("Подтвердить").clicked() {
                                confirm = Some(r.id);
                            }
                            if ui.small_button("✕").on_hover_text("Убрать замену").clicked() {
                                remove = Some(r.id);
                            }
                            if ui.small_button("другой…").clicked() {
                                select = Some((a.lesson_id, a.group));
                            }
                        });
                    }
                    None => {
                        ui.label(RichText::new("не назначено").color(theme::bad()));
                        if ui.small_button("выбрать…").clicked() {
                            select = Some((a.lesson_id, a.group));
                        }
                    }
                }
                ui.end_row();
            }
        });

        let day_reps: Vec<Replacement> = p.replacements.iter().filter(|r| r.date == date).cloned().collect();
        let issues = check_replacements(&ctx, &p.replacements);
        let lesson_ids: HashSet<Id> = day_reps.iter().map(|r| r.lesson).collect();
        let day_issues: Vec<_> = issues.iter().filter(|i| i.lessons.iter().any(|l| lesson_ids.contains(l))).collect();
        ui.add_space(10.0);
        widgets::card(ui, |ui| {
            ui.label(theme::bold("Проверка замен"));
            if day_issues.is_empty() {
                ui.label(RichText::new("Совпадений и конфликтов нет").color(theme::ok()));
            }
            for i in day_issues {
                ui.label(
                    RichText::new(format!("{} {}", if i.severity == Severity::Error { "✕" } else { "!" }, i.message)).color(if i.severity == Severity::Error {
                        theme::bad()
                    } else {
                        theme::warn()
                    }),
                );
            }
        });
        ui.add_space(6.0);
        egui::CollapsingHeader::new("Табель замен за месяц").show(ui, |ui| {
            let from = NaiveDate::from_ymd_opt(date.year(), date.month(), 1).unwrap();
            let to =
                if date.month() == 12 { NaiveDate::from_ymd_opt(date.year() + 1, 1, 1) } else { NaiveDate::from_ymd_opt(date.year(), date.month() + 1, 1) }
                    .unwrap()
                    .pred_opt()
                    .unwrap();
            let st = stats(&ctx, &p.replacements, from, to);
            if st.is_empty() {
                ui.label(theme::small("Замен в этом месяце нет."));
            }
            egui::Grid::new("tabel").striped(true).num_columns(3).show(ui, |ui| {
                if !st.is_empty() {
                    ui.label(theme::bold("Учитель"));
                    ui.label(theme::bold("Провёл замен"));
                    ui.label(theme::bold("Его уроков заменено"));
                    ui.end_row();
                }
                for x in st {
                    ui.label(ix.teacher_name(x.teacher));
                    ui.label(x.substituted.to_string());
                    ui.label(x.replaced.to_string());
                    ui.end_row();
                }
            });
        });
    });
    if let Some(s) = select {
        app.subs_ui.selected = Some(s);
    }
    if let Some(id) = remove {
        app.edit(|p| p.replacements.retain(|r| r.id != id));
    }
    if let Some(id) = confirm {
        app.edit(|p| {
            p.replacements.iter_mut().filter(|r| r.id == id).for_each(|r| {
                r.confirmed = true;
                r.stale = false;
            })
        });
    }
}

fn suggestions(app: &mut SkyApp, ui: &mut egui::Ui) {
    let Some((lesson_id, group)) = app.subs_ui.selected else {
        ui.add_space(20.0);
        ui.label(theme::small("Выберите урок в списке — здесь появятся варианты замены с объяснением."));
        return;
    };
    let p = app.project.clone();
    let date = app.subs_ui.date;
    let ix = Index::new(&p.school);
    let ctx = SubContext::new(&p.school, &p.timetable, &p.absences).with_classes(&p.class_absences);
    let Some(a) = ctx.affected(date, date).into_iter().find(|a| a.lesson_id == lesson_id && a.group == group) else {
        app.subs_ui.selected = None;
        return;
    };
    // текущая замена этого урока не должна мешать подбору
    let others: Vec<&Replacement> = p.replacements.iter().filter(|r| !(r.date == date && r.lesson == lesson_id && r.group == group)).collect();
    let policy = SubPolicy { min_auto_score: 20, allow_cancel_edge: true, allow_day_off_teachers: true };
    let list = ctx.suggest(&a, &others, &policy);
    let l = &p.timetable.lessons[a.lesson_idx];
    let load = ix.load(l.load).unwrap();
    ui.label(
        RichText::new(format!("{} урок · {} · {}", a.slot.period + 1, ix.classes_label(&load.classes), ix.subject_name(load.groups[group].subject))).heading(),
    );
    ui.label(theme::small(format!("Вместо: {}", ix.teacher_name(a.teacher))));
    ui.separator();
    let max = list.iter().map(|s| s.score).max().unwrap_or(1).max(1);
    let mut pick = None;
    egui::ScrollArea::vertical().id_salt("sugg").show(ui, |ui| {
        for (k, s) in list.iter().enumerate().take(25) {
            widgets::card(ui, |ui| {
                ui.horizontal(|ui| {
                    let who = match &s.kind {
                        ReplacementKind::Substitute { teacher, .. } => ix.teacher_name(*teacher),
                        ReplacementKind::Merge { .. } => "Объединение".into(),
                        ReplacementKind::Cancel => "Снять урок".into(),
                    };
                    ui.label(theme::bold(who));
                    let frac = (s.score.max(0) as f32 / max as f32).clamp(0.0, 1.0);
                    ui.add(egui::ProgressBar::new(frac).desired_width(80.0).text(format!("{}", s.score)));
                    if ui.button(if k == 0 { theme::bold("Назначить") } else { RichText::new("Назначить") }).clicked() {
                        pick = Some(s.clone());
                    }
                });
                for r in &s.reasons {
                    ui.label(RichText::new(format!("+ {r}")).size(12.5).color(theme::ok()));
                }
                for w in &s.warnings {
                    ui.label(RichText::new(format!("⚠ {w}")).size(12.5).color(theme::warn()));
                }
            });
        }
    });
    if let Some(s) = pick {
        assign(app, &a, s.kind, s.room, s.reasons.first().cloned().unwrap_or_default());
    }
}

fn assign(app: &mut SkyApp, a: &AffectedLesson, kind: ReplacementKind, room: Option<Id>, note: String) {
    let original = app.project.timetable.lessons.get(a.lesson_idx).and_then(|l| l.rooms.get(a.group).copied());
    let a = a.clone();
    app.edit(|p| {
        p.replacements.retain(|r| !(r.date == a.date && r.lesson == a.lesson_id && r.group == a.group));
        let id = widgets::next_id(&p.replacements, |r| r.id);
        p.replacements.push(Replacement {
            id,
            date: a.date,
            lesson: a.lesson_id,
            group: a.group,
            original_teacher: a.teacher,
            kind,
            room: if room == original { None } else { room },
            note,
            confirmed: true,
            anchor: None,
            stale: false,
        });
    });
    app.notify("Замена назначена", false);
}
