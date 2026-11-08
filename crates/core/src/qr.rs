// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
//! QR-код для ссылки на страницу замен: матрица модулей и SVG.

use qrcode::{Color, EcLevel, QrCode};

/// Квадратная матрица модулей QR-кода (true — тёмный), без полей.
pub struct QrMatrix {
    pub size: usize,
    pub dark: Vec<bool>,
}

impl QrMatrix {
    pub fn get(&self, x: usize, y: usize) -> bool {
        self.dark[y * self.size + x]
    }
}

pub fn matrix(text: &str) -> Option<QrMatrix> {
    let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M).ok()?;
    let size = code.width();
    let dark = code.to_colors().into_iter().map(|c| c == Color::Dark).collect();
    Some(QrMatrix { size, dark })
}

/// SVG с полем в 4 модуля (как требует стандарт), размер `px` на `px`.
pub fn svg(text: &str, px: u32) -> Option<String> {
    let m = matrix(text)?;
    let n = m.size + 8;
    let mut path = String::new();
    for y in 0..m.size {
        for x in 0..m.size {
            if m.get(x, y) {
                path.push_str(&format!("M{} {}h1v1h-1z", x + 4, y + 4));
            }
        }
    }
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {n} {n}\" width=\"{px}\" height=\"{px}\" shape-rendering=\"crispEdges\" role=\"img\" aria-label=\"QR-код: {}\"><rect width=\"{n}\" height=\"{n}\" fill=\"#fff\"/><path d=\"{path}\" fill=\"#000\"/></svg>",
        text.replace('"', "&quot;").replace('<', "&lt;")
    ))
}
