// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
//! SkyUrok — открытый составитель школьного расписания и замен.
//!
//! Модули:
//! - [`model`] — данные школы и проекта (один JSON-файл со схемой и версией);
//! - [`solver`] — генератор расписания;
//! - [`validate`] — проверка исходных данных и анализ расписания (конфликты, «совпадения», штрафы);
//! - [`substitute`] — отсутствия, ранжированный подбор замен, автоподбор, проверка замен, табель;
//! - [`export`] — печать расписания и отдельно листов замен (HTML), CSV;
//! - [`rtt`] — перенос данных из файлов .rtt;
//! - [`demo`] — демонстрационная школа.

pub mod demo;
pub mod export;
pub mod export_docx;
pub mod export_pdf;
pub mod grid;
pub mod history;
pub mod manual;
pub mod model;
pub mod printdoc;
pub mod qr;
pub mod rtt;
pub mod score;
pub mod solver;
pub mod substitute;
pub mod table_import;
pub mod validate;

pub use model::*;
