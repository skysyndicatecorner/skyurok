// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Обучение: мастер на 5 шагов и чек-лист готовности в левом меню.

use crate::app::{SkyApp, Tab};
use crate::theme;
use eframe::egui::{self, RichText};

#[derive(Default)]
pub struct State {
    open: bool,
    step: usize,
}

impl State {
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn open_at(&mut self, step: usize) {
        self.open = true;
        self.step = step.min(4);
    }
}

struct Step {
    title: &'static str,
    lead: &'static str,
    todo: [&'static str; 3],
    tip: &'static str,
    tab: Tab,
}

const STEPS: [Step; 5] = [
    Step {
        title: "Школа",
        lead: "Сколько учебных дней и уроков, какие здания и когда звонки. С этого программа узнаёт размер сетки.",
        todo: ["Укажите дни недели и число уроков", "Добавьте здания и заполните звонки", "Отметьте, хватает ли перемены на переход между зданиями"],
        tip: "Если зданий два, укажите, сколько минут между ними идти: программа не поставит учителю уроки подряд в разных зданиях.",
        tab: Tab::School,
    },
    Step {
        title: "Списки",
        lead: "Классы, учителя, предметы и кабинеты. Удобнее всего вставить их списком прямо из Excel или Word.",
        todo: [
            "Вставьте классы и учителей кнопкой «Добавить списком»",
            "Для каждого учителя отметьте предметы, которые он может вести",
            "Отметьте спецкабинеты метками: спортзал, химия, информатика",
        ],
        tip: "Предметы учителя нужны не только для расписания: по ним программа подбирает замены.",
        tab: Tab::Lists,
    },
    Step {
        title: "Нагрузки",
        lead: "Кто, что и сколько часов ведёт в каком классе. Здесь же деление на группы и объединение классов в поток.",
        todo: ["Выберите класс и добавьте предметы с часами", "Разделите английский и информатику на группы", "Проверьте, что часы помещаются в неделю класса"],
        tip: "Над таблицей видно, сколько часов у класса и сколько помещается в сетку. Красный — не поместится.",
        tab: Tab::Loads,
    },
    Step {
        title: "Расписание",
        lead: "Нажмите «Составить» — первый вариант будет готов за 15–20 секунд. Потом доведите его руками.",
        todo: [
            "Составьте первый вариант и сравните его с текущим",
            "Перенесите урок мышью или клавишами: стрелки, M, Enter",
            "Закрепите удачные уроки (P) и нажмите «Улучшить»",
        ],
        tip: "Ctrl+K открывает поиск: начните печатать фамилию учителя или класс, и программа покажет его расписание.",
        tab: Tab::Timetable,
    },
    Step {
        title: "Замены",
        lead: "Каждое утро: отметили, кто не пришёл, — программа предложила замены, вы подтвердили и напечатали лист.",
        todo: [
            "Отметьте отсутствующего учителя или класс",
            "Нажмите «Подобрать замены на день»",
            "Подтвердите черновики и выгрузите лист замен в Word или PDF",
        ],
        tip: "У каждого варианта замены видно, почему он предложен: «ведёт этот предмет», «у него окно», «то же здание».",
        tab: Tab::Subs,
    },
];

/// Что уже сделано — по данным проекта.
pub fn progress(app: &SkyApp) -> [bool; 5] {
    let s = &app.project.school;
    [
        s.buildings.iter().any(|b| !b.bells.is_empty()) && s.settings.periods_per_day > 0,
        !s.classes.is_empty() && !s.teachers.is_empty() && !s.subjects.is_empty(),
        !s.loads.is_empty(),
        !app.project.timetable.lessons.is_empty(),
        !app.project.replacements.is_empty(),
    ]
}

pub fn first_incomplete(app: &SkyApp) -> usize {
    progress(app).iter().position(|x| !x).unwrap_or(0)
}

pub fn nav_card(app: &mut SkyApp, ui: &mut egui::Ui) {
    let pr = progress(app);
    let done = pr.iter().filter(|x| **x).count();
    if done == 5 && app.onboarding_seen {
        return;
    }
    egui::Frame::new().fill(theme::sel_bg()).corner_radius(8.0).inner_margin(10.0).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.label(theme::bold(format!("Обучение: {done} из 5")).color(theme::sel_text()));
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 6.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 3.0, theme::surface());
        let mut r = rect;
        r.set_width(rect.width() * done as f32 / 5.0);
        ui.painter().rect_filled(r, 3.0, theme::accent());
        let next = first_incomplete(app);
        if ui.link(RichText::new(format!("Дальше: {}", STEPS[next].title)).color(theme::sel_text())).clicked() {
            app.onboarding.open_at(next);
        }
    });
}

pub fn dialog(app: &mut SkyApp, ctx: &egui::Context) {
    if !app.onboarding.open {
        return;
    }
    let pr = progress(app);
    let step = app.onboarding.step;
    let st = &STEPS[step];
    let mut close = false;
    egui::Modal::new(egui::Id::unique("onboarding")).show(ctx, |ui| {
        ui.set_width(760.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(200.0);
                ui.label(theme::bold("Первые шаги").color(theme::accent()));
                ui.label(theme::small("5 шагов · около 10 минут"));
                ui.add_space(8.0);
                for (i, s) in STEPS.iter().enumerate() {
                    let mark = if pr[i] { "✓" } else { "" };
                    let text = format!("{}  {}. {}", if mark.is_empty() { "  " } else { mark }, i + 1, s.title);
                    let r = ui.add_sized([200.0, 34.0], egui::Button::new(if i == step { theme::bold(text) } else { RichText::new(text) }).selected(i == step));
                    if r.clicked() {
                        app.onboarding.step = i;
                    }
                }
                ui.add_space(10.0);
                ui.label(theme::small("Открыть снова: «Справка» → «Обучение»."));
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.label(theme::small(format!("Шаг {} из 5{}", step + 1, if pr[step] { " · уже сделано" } else { "" })));
                ui.label(RichText::new(st.title).size(26.0).family(theme::bold_family()));
                ui.add_space(4.0);
                ui.label(RichText::new(st.lead).size(15.0));
                ui.add_space(10.0);
                egui::Frame::new().fill(theme::surface2()).corner_radius(8.0).inner_margin(12.0).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(theme::bold("Что сделать"));
                    for t in st.todo {
                        ui.label(format!("•  {t}"));
                    }
                });
                ui.add_space(8.0);
                egui::Frame::new().fill(theme::sel_bg()).corner_radius(8.0).inner_margin(12.0).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(RichText::new(format!("Совет. {}", st.tip)).color(theme::sel_text()));
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(step > 0, egui::Button::new("Назад")).clicked() {
                        app.onboarding.step -= 1;
                    }
                    if ui.button(format!("Открыть раздел «{}»", st.title)).clicked() {
                        app.tab = st.tab;
                        app.welcome = false;
                        close = true;
                    }
                    let last = step == 4;
                    let label = if last { "Готово".to_string() } else { format!("Далее: {}", STEPS[step + 1].title) };
                    if ui.add(egui::Button::new(theme::bold(label).color(theme::accent_text())).fill(theme::accent())).clicked() {
                        if last {
                            close = true;
                        } else {
                            app.onboarding.step += 1;
                        }
                    }
                    if ui.link("Пропустить обучение").clicked() {
                        close = true;
                    }
                });
            });
        });
    });
    if close {
        app.onboarding.open = false;
        app.onboarding_seen = true;
    }
}
