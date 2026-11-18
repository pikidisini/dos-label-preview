use std::fmt::Write as _;

use crate::model::{ElementKind, GraphicBitmap, LabelDocument, TaxElement};

pub const DEFAULT_PAPER_WIDTH: i32 = 2177;
pub const DEFAULT_PAPER_HEIGHT: i32 = 813;

fn base_font_dots(el: &TaxElement) -> f32 {
    if let Some(pt) = el.point_size {
        return (pt.max(4) as f32) * 203.2 / 72.0;
    }
    match el.weight_code {
        2 => 20.0,  // c2 (condensed/smaller bitmap)
        3 => 26.0,  // c3
        20 => 24.0, // c20 (8-pt monospace @ 203 dpi ≈ 22.6 dots)
        21 => 26.0, // c21 (10-pt)
        22 => 32.0, // c22 (12-pt)
        _ => 24.0,
    }
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

fn code39_pattern(ch: char) -> Option<&'static str> {
    Some(match ch {
        '0' => "nnnwwnwnn",
        '1' => "wnnwnnnnw",
        '2' => "nnwwnnnnw",
        '3' => "wnwwnnnnn",
        '4' => "nnnwwnnnw",
        '5' => "wnnwwnnnn",
        '6' => "nnwwwnnnn",
        '7' => "nnnwnnwnw",
        '8' => "wnnwnnwnn",
        '9' => "nnwwnnwnn",
        'A' => "wnnnnwnnw",
        'B' => "nnwnnwnnw",
        'C' => "wnwnnwnnn",
        'D' => "nnnnwwnnw",
        'E' => "wnnnwwnnn",
        'F' => "nnwnwwnnn",
        'G' => "nnnnnwwnw",
        'H' => "wnnnnwwnn",
        'I' => "nnwnnwwnn",
        'J' => "nnnnwwwnn",
        'K' => "wnnnnnnww",
        'L' => "nnwnnnnww",
        'M' => "wnwnnnnwn",
        'N' => "nnnnwnnww",
        'O' => "wnnnwnnwn",
        'P' => "nnwnwnnwn",
        'Q' => "nnnnnnwww",
        'R' => "wnnnnnwwn",
        'S' => "nnwnnnwwn",
        'T' => "nnnnwnwwn",
        'U' => "wwnnnnnnw",
        'V' => "nwwnnnnnw",
        'W' => "wwwnnnnnn",
        'X' => "nwnnwnnnw",
        'Y' => "wwnnwnnnn",
        'Z' => "nwwnwnnnn",
        '-' => "nwnnnnwnw",
        '.' => "wwnnnnwnn",
        ' ' => "nwwnnnwnn",
        '$' => "nwnwnwnnn",
        '/' => "nwnwnnnwn",
        '+' => "nwnnnwnwn",
        '%' => "nnnwnwnwn",
        '*' => "nwnnwnwnn",
        _ => return None,
    })
}

fn encode_code39(value: &str, module: i32) -> Vec<(bool, i32)> {
    let clean: String = value
        .to_uppercase()
        .chars()
        .filter(|&c| code39_pattern(c).is_some())
        .collect();
    if clean.is_empty() {
        return Vec::new();
    }

    let encoded = format!("*{clean}*");
    let total_chars = encoded.len();
    let mut units = Vec::new();

    for (idx, ch) in encoded.chars().enumerate() {
        if let Some(pattern) = code39_pattern(ch) {
            let mut is_bar = true;
            for token in pattern.chars() {
                let width = if token == 'n' { module } else { module * 2 };
                units.push((is_bar, width));
                is_bar = !is_bar;
            }
            if idx + 1 != total_chars {
                units.push((false, module));
            }
        }
    }
    units
}

fn render_graphic_path(bmp: &GraphicBitmap, el: &TaxElement, out: &mut String) {
    if bmp.width == 0 || bmp.height == 0 {
        return;
    }

    let sx = el.w_mult.max(1) as usize;
    let sy = el.h_mult.max(1) as usize;
    let mut path_data = String::new();

    // Run-length encode horizontal black runs into a single SVG path.
    for row in 0..bmp.height {
        let row_offset = row * bmp.width;
        let mut col = 0;
        while col < bmp.width {
            if !bmp.pixels[row_offset + col] {
                col += 1;
                continue;
            }
            let start_col = col;
            while col < bmp.width && bmp.pixels[row_offset + col] {
                col += 1;
            }
            let run_w = (col - start_col) * sx;
            let px = start_col * sx;
            let py = row * sy;
            let _ = write!(path_data, "M{px},{py}h{run_w}v{sy}h-{run_w}z");
        }
    }

    if path_data.is_empty() {
        return;
    }

    let transform = if el.rotation != 0 {
        format!("translate({}, {}) rotate({})", el.x, el.y, el.rotation)
    } else {
        format!("translate({}, {})", el.x, el.y)
    };

    let _ = writeln!(
        out,
        r#"  <g id="{}" data-tax-name="{}" transform="{transform}"><path d="{path_data}" fill="black" shape-rendering="crispEdges"/></g>"#,
        xml_escape(&el.raw_id),
        xml_escape(&el.name),
    );
}

/// Render a `LabelDocument` into an SVG string.
pub fn render_to_svg(
    doc: &LabelDocument,
    canvas_width: i32,
    canvas_height: i32,
    show_var_names: bool,
) -> String {
    let w = canvas_width.max(100);
    let h = canvas_height.max(100);

    let mut out = String::with_capacity(8192);
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">"#
    );
    let _ = writeln!(
        out,
        r##"  <rect x="0" y="0" width="{w}" height="{h}" fill="white" stroke="#333" stroke-width="2" data-tax-ignore="true"/>"##
    );

    for el in &doc.elements {
        if el.raw_id == "L99" {
            continue;
        }

        match el.kind {
            ElementKind::Graphic => {
                if let Some(gid) = el.graphic_id {
                    if let Some(bmp) = doc.graphics.get(&gid) {
                        render_graphic_path(bmp, el, &mut out);
                    }
                }
            }
            ElementKind::Line => {
                let x1 = el.x;
                let y1 = el.y;
                let (x2, y2) = match el.rotation {
                    -90 => (x1, y1 - el.line_length),
                    180 => (x1 - el.line_length, y1),
                    90 => (x1, y1 + el.line_length),
                    _ => {
                        if el.line_vertical {
                            (x1, y1 + el.line_length)
                        } else {
                            (x1 + el.line_length, y1)
                        }
                    }
                };
                let _ = writeln!(
                    out,
                    r#"  <line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="black" stroke-width="{}" stroke-linecap="square" data-tax-id="{}"/>"#,
                    el.line_width.max(1),
                    xml_escape(&el.raw_id)
                );
            }
            ElementKind::Barcode => {
                let value = if show_var_names {
                    el.name.clone()
                } else {
                    el.static_text
                        .clone()
                        .unwrap_or_else(|| el.name.clone())
                };
                let units = encode_code39(&value, el.barcode_module.max(1));
                if units.is_empty() {
                    continue;
                }
                let bh = el.barcode_height.max(30);
                let mut cursor = 0;
                let mut path_data = String::new();
                for (is_bar, width) in units {
                    if is_bar {
                        let _ = write!(path_data, "M{cursor},0h{width}v{bh}h-{width}z");
                    }
                    cursor += width;
                }
                let transform = if el.rotation != 0 {
                    format!("translate({}, {}) rotate({})", el.x, el.y, el.rotation)
                } else {
                    format!("translate({}, {})", el.x, el.y)
                };
                let _ = writeln!(
                    out,
                    r#"  <g data-tax-id="{}" data-tax-name="{}" transform="{transform}"><path d="{path_data}" fill="black" shape-rendering="crispEdges"/></g>"#,
                    xml_escape(&el.raw_id),
                    xml_escape(&el.name)
                );
            }
            ElementKind::Text => {
                let display_text = if show_var_names {
                    if el.is_static_text {
                        el.static_text.clone().unwrap_or_default()
                    } else {
                        format!("[{}]", el.name)
                    }
                } else {
                    el.static_text.clone().unwrap_or_default()
                };

                if display_text.is_empty() {
                    continue;
                }

                let base_size = base_font_dots(el);
                let ascent = (base_size * 0.82).round();
                let sx = el.w_mult.max(1);
                let sy = el.h_mult.max(1);
                let font_weight = if el.weight_code == 21 { "bold" } else { "normal" };

                let transform = if el.rotation != 0 {
                    format!(
                        "translate({}, {}) rotate({}) scale({sx}, {sy})",
                        el.x, el.y, el.rotation
                    )
                } else if sx != 1 || sy != 1 {
                    format!("translate({}, {}) scale({sx}, {sy})", el.x, el.y)
                } else {
                    format!("translate({}, {})", el.x, el.y)
                };

                let _ = writeln!(
                    out,
                    r#"  <g transform="{transform}" data-tax-id="{}" data-tax-name="{}"><text x="0" y="{ascent}" font-family="Consolas, 'Courier New', monospace" font-size="{base_size:.1}" font-weight="{font_weight}" fill="black">{}</text></g>"#,
                    xml_escape(&el.raw_id),
                    xml_escape(&el.name),
                    xml_escape(&display_text)
                );
            }
        }
    }

    out.push_str("</svg>\n");
    out
}
