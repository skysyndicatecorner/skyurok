// SPDX-FileCopyrightText: 2026 skysyndicatecorner
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Запись `PrintDoc` в Word (.docx, Office Open XML) без внешних программ.

use crate::printdoc::{Mark, PrintDoc};
use std::io::Write;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Абзац; `size` в половинах пункта.
fn para(text: &str, bold: bool, size: u32, align: &str, color: Option<&str>, after: u32) -> String {
    let mut rpr = String::new();
    if bold {
        rpr.push_str("<w:b/>");
    }
    if let Some(c) = color {
        rpr.push_str(&format!("<w:color w:val=\"{c}\"/>"));
    }
    rpr.push_str(&format!("<w:sz w:val=\"{size}\"/><w:szCs w:val=\"{size}\"/>"));
    format!(
        "<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"{after}\"/><w:jc w:val=\"{align}\"/></w:pPr><w:r><w:rPr>{rpr}</w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
        esc(text)
    )
}

fn sect(landscape: bool) -> String {
    let (w, h, orient) = if landscape { (16838, 11906, " w:orient=\"landscape\"") } else { (11906, 16838, "") };
    format!(
        "<w:sectPr><w:pgSz w:w=\"{w}\" w:h=\"{h}\"{orient}/><w:pgMar w:top=\"680\" w:right=\"680\" w:bottom=\"680\" w:left=\"680\" w:header=\"340\" w:footer=\"340\" w:gutter=\"0\"/></w:sectPr>"
    )
}

pub fn write(doc: &PrintDoc) -> Result<Vec<u8>, String> {
    let mut body = String::new();
    let n = doc.pages.len();
    for (pi, page) in doc.pages.iter().enumerate() {
        let usable: f32 = if page.landscape { 16838.0 - 1360.0 } else { 11906.0 - 1360.0 };
        // шапка: школа слева, гриф справа
        if doc.approve {
            body.push_str(&format!(
                "<w:tbl><w:tblPr><w:tblW w:w=\"{}\" w:type=\"dxa\"/><w:tblBorders><w:top w:val=\"nil\"/><w:left w:val=\"nil\"/><w:bottom w:val=\"nil\"/><w:right w:val=\"nil\"/><w:insideH w:val=\"nil\"/><w:insideV w:val=\"nil\"/></w:tblBorders></w:tblPr><w:tblGrid><w:gridCol w:w=\"{}\"/><w:gridCol w:w=\"{}\"/></w:tblGrid><w:tr><w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/></w:tcPr>{}</w:tc><w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/></w:tcPr>{}{}{}</w:tc></w:tr></w:tbl>",
                usable as i32,
                (usable * 0.55) as i32,
                (usable * 0.45) as i32,
                (usable * 0.55) as i32,
                para(&doc.school, true, 20, "left", None, 0),
                (usable * 0.45) as i32,
                para("УТВЕРЖДАЮ", true, 18, "right", None, 0),
                para("Директор ____________ / ____________ /", false, 18, "right", None, 0),
                para("«____» ______________ 20___ г.", false, 18, "right", None, 120)
            ));
        } else {
            body.push_str(&para(&doc.school, true, 20, "left", None, 120));
        }
        if let Some(d) = &doc.draft {
            body.push_str(&format!(
                "<w:p><w:pPr><w:pBdr><w:top w:val=\"single\" w:sz=\"12\" w:space=\"2\" w:color=\"B42318\"/><w:left w:val=\"single\" w:sz=\"12\" w:space=\"2\" w:color=\"B42318\"/><w:bottom w:val=\"single\" w:sz=\"12\" w:space=\"2\" w:color=\"B42318\"/><w:right w:val=\"single\" w:sz=\"12\" w:space=\"2\" w:color=\"B42318\"/></w:pBdr><w:spacing w:after=\"120\"/></w:pPr><w:r><w:rPr><w:b/><w:color w:val=\"B42318\"/><w:sz w:val=\"20\"/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
                esc(d)
            ));
        }
        body.push_str(&para(&page.title, true, 30, "center", None, 40));
        if !page.subtitle.is_empty() {
            body.push_str(&para(&page.subtitle, false, 20, "center", Some("57606A"), 120));
        }
        if let Some(note) = &page.note {
            body.push_str(&para(note, false, 20, "left", None, 120));
        }
        // таблица
        let t = &page.table;
        let total: f32 = t.widths.iter().sum();
        let cols: Vec<i32> = t.widths.iter().map(|w| (usable * w / total) as i32).collect();
        let big = t.rows.len() <= 10 && !page.landscape;
        let fsz = if page.landscape {
            16
        } else if big {
            20
        } else {
            18
        };
        body.push_str("<w:tbl><w:tblPr>");
        body.push_str(&format!("<w:tblW w:w=\"{}\" w:type=\"dxa\"/><w:tblLayout w:type=\"fixed\"/>", usable as i32));
        body.push_str("<w:tblBorders><w:top w:val=\"single\" w:sz=\"6\" w:color=\"8C959F\"/><w:left w:val=\"single\" w:sz=\"6\" w:color=\"8C959F\"/><w:bottom w:val=\"single\" w:sz=\"6\" w:color=\"8C959F\"/><w:right w:val=\"single\" w:sz=\"6\" w:color=\"8C959F\"/><w:insideH w:val=\"single\" w:sz=\"4\" w:color=\"8C959F\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:color=\"8C959F\"/></w:tblBorders>");
        body.push_str("<w:tblCellMar><w:top w:w=\"40\" w:type=\"dxa\"/><w:left w:w=\"70\" w:type=\"dxa\"/><w:bottom w:w=\"40\" w:type=\"dxa\"/><w:right w:w=\"70\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr><w:tblGrid>");
        for c in &cols {
            body.push_str(&format!("<w:gridCol w:w=\"{c}\"/>"));
        }
        body.push_str("</w:tblGrid>");
        // заголовок, повторяется на каждой странице
        body.push_str("<w:tr><w:trPr><w:tblHeader/><w:cantSplit/></w:trPr>");
        for (i, h) in t.header.iter().enumerate() {
            let shade = if doc.grayscale { String::new() } else { "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"EEF2F6\"/>".into() };
            body.push_str(&format!(
                "<w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/>{shade}</w:tcPr>{}</w:tc>",
                cols.get(i).copied().unwrap_or(1000),
                para(h, true, fsz, "center", None, 0)
            ));
        }
        body.push_str("</w:tr>");
        for row in &t.rows {
            body.push_str("<w:tr><w:trPr><w:cantSplit/></w:trPr>");
            for (i, cell) in row.iter().enumerate() {
                let fill = match (doc.grayscale, cell.mark) {
                    (true, _) | (_, Mark::None) => None,
                    (_, Mark::Header) => Some("EEF2F6"),
                    (_, Mark::Problem) => Some("FDECEA"),
                    (_, Mark::Replacement) => Some("FFE9B8"),
                    (_, Mark::Cancel) => Some("FDECEA"),
                };
                let shade = fill.map(|f| format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{f}\"/>")).unwrap_or_default();
                let mut inner = String::new();
                if cell.lines.is_empty() {
                    inner.push_str("<w:p/>");
                }
                for (k, line) in cell.lines.iter().enumerate() {
                    let bold = k == 0 && cell.bold_first;
                    let color = if cell.mark == Mark::Problem && k == 0 {
                        Some("B42318")
                    } else if k > 0 {
                        Some("3D444D")
                    } else {
                        None
                    };
                    inner.push_str(&para(line, bold, if k == 0 { fsz } else { fsz - 2 }, "left", color, 0));
                }
                body.push_str(&format!("<w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/>{shade}</w:tcPr>{inner}</w:tc>", cols.get(i).copied().unwrap_or(1000)));
            }
            body.push_str("</w:tr>");
        }
        body.push_str("</w:tbl>");
        body.push_str(&para("Составлено в SkyUrok", false, 14, "right", Some("57606A"), 0));
        if pi + 1 < n {
            // конец раздела = новая страница; ориентация — этой страницы
            body.push_str(&format!("<w:p><w:pPr>{}</w:pPr></w:p>", sect(page.landscape)));
        }
    }
    let last_land = doc.pages.last().is_some_and(|p| p.landscape);
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><w:body>{}{}</w:body></w:document>",
        body,
        sect(last_land)
    );
    let styles = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Arial" w:hAnsi="Arial" w:cs="Arial" w:eastAsia="Arial"/><w:sz w:val="20"/><w:szCs w:val="20"/><w:lang w:val="ru-RU"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style></w:styles>"#;
    let content_types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#;
    let doc_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;
    let core = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>{}</dc:title><dc:creator>SkyUrok</dc:creator></cp:coreProperties>"#,
        esc(&format!("{} — {}", doc.title, doc.school))
    );

    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in [
            ("[Content_Types].xml", content_types.to_string()),
            ("_rels/.rels", rels.to_string()),
            ("word/_rels/document.xml.rels", doc_rels.to_string()),
            ("word/document.xml", document),
            ("word/styles.xml", styles.to_string()),
            ("docProps/core.xml", core),
        ] {
            z.start_file(name, opt).map_err(|e| e.to_string())?;
            z.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
        }
        z.finish().map_err(|e| e.to_string())?;
    }
    Ok(buf.into_inner())
}
