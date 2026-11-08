// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Запись `PrintDoc` в PDF (A4) со встроенным шрифтом DejaVu Sans (кириллица).

use crate::printdoc::{DocCell, DocPage, Mark, PrintDoc};
use printpdf::*;

pub static FONT_REGULAR: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");
pub static FONT_BOLD: &[u8] = include_bytes!("../assets/DejaVuSans-Bold.ttf");

const PT: f32 = 0.352_778; // мм в пункте
const MARGIN: f32 = 12.0;

struct Metrics {
    reg: ttf_parser::Face<'static>,
    bold: ttf_parser::Face<'static>,
}

impl Metrics {
    fn width(&self, s: &str, size: f32, bold: bool) -> f32 {
        let f = if bold { &self.bold } else { &self.reg };
        let upem = f.units_per_em() as f32;
        let adv: f32 = s.chars().map(|c| f.glyph_index(c).and_then(|g| f.glyph_hor_advance(g)).unwrap_or((upem * 0.5) as u16) as f32).sum();
        adv / upem * size * PT
    }

    /// Перенос строки по словам в ширину `w` мм.
    fn wrap(&self, s: &str, size: f32, bold: bool, w: f32) -> Vec<String> {
        let mut out = vec![];
        let mut cur = String::new();
        for word in s.split(' ') {
            let cand = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
            if self.width(&cand, size, bold) <= w || cur.is_empty() {
                cur = cand;
                // слово длиннее ячейки — режем по символам
                while self.width(&cur, size, bold) > w && cur.chars().count() > 1 {
                    let mut head = String::new();
                    for ch in cur.chars() {
                        if self.width(&format!("{head}{ch}"), size, bold) > w {
                            break;
                        }
                        head.push(ch);
                    }
                    if head.is_empty() {
                        break;
                    }
                    let rest: String = cur.chars().skip(head.chars().count()).collect();
                    out.push(head);
                    cur = rest;
                }
            } else {
                out.push(std::mem::take(&mut cur));
                cur = word.to_string();
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
        out
    }
}

struct Ctx<'a> {
    doc: &'a PdfDocumentReference,
    reg: IndirectFontRef,
    bold: IndirectFontRef,
    m: Metrics,
    grayscale: bool,
    page_no: usize,
}

struct Canvas {
    layer: PdfLayerReference,
    w: f32,
    h: f32,
    y: f32, // от верхнего края, мм
}

fn rgb(hex: u32) -> Color {
    Color::Rgb(Rgb::new(((hex >> 16) & 255) as f32 / 255.0, ((hex >> 8) & 255) as f32 / 255.0, (hex & 255) as f32 / 255.0, None))
}

impl Canvas {
    fn text(&self, c: &Ctx, s: &str, size: f32, x: f32, y_top: f32, bold: bool, color: u32) {
        self.layer.set_fill_color(rgb(color));
        self.layer.use_text(s, size, Mm(x), Mm(self.h - y_top - size * PT * 0.8), if bold { &c.bold } else { &c.reg });
    }
    fn rect(&self, x: f32, y_top: f32, w: f32, h: f32, fill: Option<u32>, stroke: Option<(u32, f32)>) {
        let r = Rect::new(Mm(x), Mm(self.h - y_top - h), Mm(x + w), Mm(self.h - y_top));
        if let Some(f) = fill {
            self.layer.set_fill_color(rgb(f));
            self.layer.add_rect(r.clone().with_mode(path::PaintMode::Fill));
        }
        if let Some((s, t)) = stroke {
            self.layer.set_outline_color(rgb(s));
            self.layer.set_outline_thickness(t);
            self.layer.add_rect(r.with_mode(path::PaintMode::Stroke));
        }
    }
}

fn new_canvas(c: &mut Ctx, landscape: bool, first: Option<(PdfPageIndex, PdfLayerIndex)>) -> Canvas {
    let (w, h) = if landscape { (297.0, 210.0) } else { (210.0, 297.0) };
    let (pi, li) = match first {
        Some(x) => x,
        None => c.doc.add_page(Mm(w), Mm(h), "Слой"),
    };
    c.page_no += 1;
    let layer = c.doc.get_page(pi).get_layer(li);
    let cv = Canvas { layer, w, h, y: MARGIN };
    // нижний колонтитул
    let foot = format!("SkyUrok · стр. {}", c.page_no);
    let fw = c.m.width(&foot, 7.0, false);
    cv.text(c, &foot, 7.0, w - MARGIN - fw, h - MARGIN + 2.0, false, 0x57606A);
    cv
}

fn page_header(c: &Ctx, cv: &mut Canvas, doc: &PrintDoc, page: &DocPage, cont: bool) {
    cv.text(c, &doc.school, 9.0, MARGIN, cv.y, true, 0x1B1F24);
    if doc.approve && !cont {
        for (k, line) in ["УТВЕРЖДАЮ", "Директор ____________ / ____________ /", "«____» ______________ 20___ г."].iter().enumerate() {
            let fw = c.m.width(line, 8.0, k == 0);
            cv.text(c, line, 8.0, cv.w - MARGIN - fw, cv.y + k as f32 * 4.0, k == 0, 0x1B1F24);
        }
        cv.y += 14.0;
    } else {
        cv.y += 6.0;
    }
    if let Some(d) = &doc.draft {
        let lines = c.m.wrap(d, 8.5, true, cv.w - 2.0 * MARGIN - 6.0);
        let hgt = lines.len() as f32 * 4.2 + 3.0;
        cv.rect(MARGIN, cv.y, cv.w - 2.0 * MARGIN, hgt, if c.grayscale { None } else { Some(0xFDECEA) }, Some((0xB42318, 1.2)));
        for (k, l) in lines.iter().enumerate() {
            cv.text(c, l, 8.5, MARGIN + 3.0, cv.y + 1.6 + k as f32 * 4.2, true, 0xB42318);
        }
        cv.y += hgt + 3.0;
        // диагональный водяной знак
        cv.layer.set_fill_color(rgb(if c.grayscale { 0xE6E6E6 } else { 0xF6D5D2 }));
        cv.layer.begin_text_section();
        cv.layer.set_font(&c.bold, 72.0);
        cv.layer.set_text_matrix(TextMatrix::TranslateRotate(Mm(cv.w * 0.18).into(), Mm(cv.h * 0.28).into(), 35.0));
        cv.layer.write_text("ЧЕРНОВИК", &c.bold);
        cv.layer.end_text_section();
    }
    let title = if cont { format!("{} (продолжение)", page.title) } else { page.title.clone() };
    let tw = c.m.width(&title, 14.0, true);
    cv.text(c, &title, 14.0, (cv.w - tw) / 2.0, cv.y, true, 0x1B1F24);
    cv.y += 7.0;
    if !page.subtitle.is_empty() && !cont {
        let sw = c.m.width(&page.subtitle, 9.0, false);
        cv.text(c, &page.subtitle, 9.0, (cv.w - sw) / 2.0, cv.y, false, 0x57606A);
        cv.y += 5.5;
    }
    if let (Some(n), false) = (&page.note, cont) {
        for l in c.m.wrap(n, 9.0, false, cv.w - 2.0 * MARGIN) {
            cv.text(c, &l, 9.0, MARGIN, cv.y, false, 0x1B1F24);
            cv.y += 4.5;
        }
    }
    cv.y += 2.0;
}

struct Laid {
    lines: Vec<(String, bool, f32)>, // текст, жирный, кегль
    h: f32,
}

fn lay_cell(c: &Ctx, cell: &DocCell, w: f32, size: f32) -> Laid {
    let mut lines = vec![];
    for (k, l) in cell.lines.iter().enumerate() {
        let bold = k == 0 && cell.bold_first;
        let sz = if k == 0 { size } else { size - 1.0 };
        for wl in c.m.wrap(l, sz, bold, w - 2.4) {
            lines.push((wl, bold, sz));
        }
    }
    let h: f32 = lines.iter().map(|x| x.2 * PT * 1.25).sum::<f32>() + 2.4;
    Laid { lines, h }
}

fn draw_row(c: &Ctx, cv: &Canvas, xs: &[f32], ws: &[f32], laid: &[Laid], marks: &[Mark], h: f32) {
    for (i, l) in laid.iter().enumerate() {
        let fill = match (c.grayscale, marks.get(i).copied().unwrap_or(Mark::None)) {
            (true, _) | (_, Mark::None) => None,
            (_, Mark::Header) => Some(0xEEF2F6),
            (_, Mark::Problem) | (_, Mark::Cancel) => Some(0xFDECEA),
            (_, Mark::Replacement) => Some(0xFFE9B8),
        };
        let problem = marks.get(i) == Some(&Mark::Problem);
        cv.rect(xs[i], cv.y, ws[i], h, fill, Some(if problem { (0xB42318, 1.0) } else { (0x8C959F, 0.4) }));
        let mut y = cv.y + 1.2;
        for (k, (t, b, sz)) in l.lines.iter().enumerate() {
            let color = if problem && k == 0 {
                0xB42318
            } else if k > 0 && !*b {
                0x3D444D
            } else {
                0x1B1F24
            };
            cv.text(c, t, *sz, xs[i] + 1.2, y, *b, color);
            y += sz * PT * 1.25;
        }
    }
}

pub fn write(doc: &PrintDoc) -> Result<Vec<u8>, String> {
    let first_land = doc.pages.first().is_some_and(|p| p.landscape);
    let (w0, h0) = if first_land { (297.0, 210.0) } else { (210.0, 297.0) };
    let (pdf, p0, l0) = PdfDocument::new(format!("{} — {}", doc.title, doc.school), Mm(w0), Mm(h0), "Слой");
    let reg = pdf.add_external_font_with_subsetting(FONT_REGULAR, true).map_err(|e| e.to_string())?;
    let bold = pdf.add_external_font_with_subsetting(FONT_BOLD, true).map_err(|e| e.to_string())?;
    let m = Metrics {
        reg: ttf_parser::Face::parse(FONT_REGULAR, 0).map_err(|e| e.to_string())?,
        bold: ttf_parser::Face::parse(FONT_BOLD, 0).map_err(|e| e.to_string())?,
    };
    let mut c = Ctx { doc: &pdf, reg, bold, m, grayscale: doc.grayscale, page_no: 0 };
    let mut first = Some((p0, l0));
    for page in &doc.pages {
        let mut cv = new_canvas(&mut c, page.landscape, first.take());
        page_header(&c, &mut cv, doc, page, false);
        let t = &page.table;
        let total: f32 = t.widths.iter().sum();
        let avail = cv.w - 2.0 * MARGIN;
        let ws: Vec<f32> = t.widths.iter().map(|w| avail * w / total).collect();
        let xs: Vec<f32> = ws
            .iter()
            .scan(MARGIN, |acc, w| {
                let x = *acc;
                *acc += w;
                Some(x)
            })
            .collect();
        // кегль: подбираем, чтобы таблица уроков влезла на страницу
        let mut size = if page.landscape {
            7.5
        } else if t.rows.len() <= 10 {
            9.5
        } else {
            8.5
        };
        let header_cells: Vec<DocCell> = t.header.iter().map(|h| DocCell::bold(h.clone())).collect();
        loop {
            let hh: f32 = header_cells.iter().zip(&ws).map(|(cl, w)| lay_cell(&c, cl, *w, size).h).fold(0.0, f32::max);
            let rows_h: f32 = t.rows.iter().map(|r| r.iter().zip(&ws).map(|(cl, w)| lay_cell(&c, cl, *w, size).h).fold(0.0, f32::max)).sum();
            if hh + rows_h <= cv.h - cv.y - MARGIN - 6.0 || size <= 6.5 || t.rows.len() > 40 {
                break;
            }
            size -= 0.5;
        }
        let header_laid: Vec<Laid> = header_cells.iter().zip(&ws).map(|(cl, w)| lay_cell(&c, cl, *w, size)).collect();
        let hh = header_laid.iter().map(|l| l.h).fold(0.0, f32::max);
        let hmarks = vec![Mark::Header; ws.len()];
        draw_row(&c, &cv, &xs, &ws, &header_laid, &hmarks, hh);
        cv.y += hh;
        for row in &t.rows {
            let laid: Vec<Laid> = row.iter().zip(&ws).map(|(cl, w)| lay_cell(&c, cl, *w, size)).collect();
            let rh = laid.iter().map(|l| l.h).fold(0.0, f32::max);
            if cv.y + rh > cv.h - MARGIN - 5.0 {
                cv = new_canvas(&mut c, page.landscape, None);
                page_header(&c, &mut cv, doc, page, true);
                draw_row(&c, &cv, &xs, &ws, &header_laid, &hmarks, hh);
                cv.y += hh;
            }
            let marks: Vec<Mark> = row.iter().map(|x| x.mark).collect();
            draw_row(&c, &cv, &xs, &ws, &laid, &marks, rh);
            cv.y += rh;
        }
    }
    pdf.save_to_bytes().map_err(|e| e.to_string())
}
