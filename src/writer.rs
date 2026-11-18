use std::collections::{HashMap, HashSet};

use once_cell::sync::Lazy;
use regex::Regex;

use crate::model::{ElementKind, GraphicBitmap, LabelDocument, TaxElement};
use crate::parser::normalize_key;

static DAX_DATA_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"^\x02\x1BF"([^"]+)"\x0A([^\x03]*)\x03$"#).unwrap());

/// Convert raw IPL control bytes back into human-readable ASCII tags (`<STX>`, `<ETX>`, etc.).
pub fn restore_ascii_tags(input: &str) -> String {
    input
        .replace('\x02', "<STX>")
        .replace('\x03', "<ETX>")
        .replace('\x1B', "<ESC>")
        .replace('\x0A', "<LF>")
        .replace('\x1E', "<RS>")
        .replace('\x1F', "<US>")
        .replace('\x17', "<ETB>")
        .replace('\x18', "<CAN>")
}

fn rotation_to_font_dir(rotation: i32) -> i32 {
    match rotation {
        -90 => 1,
        180 => 2,
        90 => 3,
        _ => 0,
    }
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_origin_token(tok: &str) -> bool {
    let Some(rest) = tok.strip_prefix('o') else {
        return false;
    };
    let Some((x, y)) = rest.split_once(',') else {
        return false;
    };
    is_digits(x) && is_digits(y)
}

fn patch_attr_token(tok: &str, el: &TaxElement) -> String {
    if is_origin_token(tok) {
        return format!("o{},{}", el.x, el.y);
    }
    if let Some(rest) = tok.strip_prefix('f') {
        if is_digits(rest) {
            return format!("f{}", rotation_to_font_dir(el.rotation));
        }
    }
    if let Some(rest) = tok.strip_prefix('h') {
        if is_digits(rest) {
            let h_val = if el.kind == ElementKind::Barcode {
                el.barcode_height
            } else {
                el.h_mult
            };
            return format!("h{h_val}");
        }
    }
    if let Some(rest) = tok.strip_prefix('w') {
        if is_digits(rest) {
            let w_val = match el.kind {
                ElementKind::Barcode => el.barcode_module,
                ElementKind::Line | ElementKind::Box => el.line_width,
                _ => el.w_mult,
            };
            return format!("w{w_val}");
        }
    }
    if let Some(rest) = tok.strip_prefix('c') {
        if is_digits(rest) {
            let c_val = if el.kind == ElementKind::Graphic {
                el.graphic_id
                    .map(|g| g as i32)
                    .unwrap_or(el.weight_code)
            } else {
                el.weight_code
            };
            return format!("c{c_val}");
        }
    }
    if matches!(el.kind, ElementKind::Line | ElementKind::Box) {
        if let Some(rest) = tok.strip_prefix('l') {
            if is_digits(rest) {
                return format!("l{}", el.line_length);
            }
        }
    }
    tok.to_string()
}

fn patch_layout_line(raw_line: &str, el: &TaxElement) -> String {
    let trimmed = raw_line.trim();
    let Some(etx_pos) = trimmed.rfind('\x03') else {
        return raw_line.to_string();
    };
    let Some(semi_pos) = trimmed[..etx_pos].find(';') else {
        return raw_line.to_string();
    };

    let header = &trimmed[..=semi_pos];
    let body = &trimmed[semi_pos + 1..etx_pos];

    // Split off trailing `d3,<static text>` if present so semicolons inside static text are safe.
    let d3_start = if body.starts_with("d3,") {
        Some(0)
    } else {
        body.find(";d3,").map(|i| i + 1)
    };

    let (attr_part, has_d3) = match d3_start {
        Some(idx) => (&body[..idx], true),
        None => (body, false),
    };

    let mut tokens: Vec<String> = attr_part
        .split(';')
        .map(|tok| patch_attr_token(tok, el))
        .collect();

    if el.is_static_text {
        if let Some(ref txt) = el.static_text {
            // Replace or append `d3,<text>;`
            if tokens.last().is_some_and(|t| t.is_empty()) {
                tokens.pop();
            }
            tokens.push(format!("d3,{txt}"));
            tokens.push(String::new());
        } else if has_d3 {
            // Preserve empty d3 if static_text was cleared
            if tokens.last().is_some_and(|t| t.is_empty()) {
                tokens.pop();
            }
            tokens.push("d3,".to_string());
            tokens.push(String::new());
        }
    }

    let updated_body = tokens.join(";");
    format!("{header}{updated_body}\x03")
}

fn encode_ipl_6bit_graphic(gid: u32, bmp: &GraphicBitmap) -> Vec<String> {
    let mut out = Vec::with_capacity(bmp.width + 1);
    out.push(format!("\x02G{gid},0002;x{};y{};\x03", bmp.width, bmp.height));
    let chars_per_col = (bmp.height + 5) / 6;
    for col in 0..bmp.width {
        let mut col_str = String::with_capacity(chars_per_col);
        for char_idx in 0..chars_per_col {
            let mut bits: u8 = 0;
            for bit in 0..6 {
                let row = char_idx * 6 + bit;
                if row < bmp.height && bmp.pixels[row * bmp.width + col] {
                    bits |= 1 << bit;
                }
            }
            let ch = if bits == 0b11_1111 {
                ' '
            } else {
                (0x40 + bits) as char
            };
            col_str.push(ch);
        }
        out.push(format!("\x02u{col},{col_str};\x03"));
    }
    out
}

fn format_new_element_line(el: &TaxElement) -> String {
    let id = el
        .raw_id
        .trim_start_matches(|c: char| c.is_ascii_alphabetic());
    let id = if id.is_empty() { "1" } else { id };
    let f_dir = rotation_to_font_dir(el.rotation);

    match el.kind {
        ElementKind::Text => {
            let data_part = if el.is_static_text {
                format!("d3,{}", el.static_text.as_deref().unwrap_or(""))
            } else {
                let len = el.dax_value.as_ref().map(|v| v.len().max(1)).unwrap_or(20);
                format!("d0,{len}")
            };
            format!(
                "\x02H{id},{};o{},{};f{f_dir};h{};w{};c{};b0;{data_part};\x03",
                el.name,
                el.x,
                el.y,
                el.h_mult.max(1),
                el.w_mult.max(1),
                el.weight_code
            )
        }
        ElementKind::Barcode => {
            let data_part = if el.is_static_text {
                format!("d3,{}", el.static_text.as_deref().unwrap_or(""))
            } else {
                let len = el.dax_value.as_ref().map(|v| v.len().max(1)).unwrap_or(20);
                format!("d0,{len}")
            };
            if el.barcode_type.as_deref() == Some("QR") {
                format!(
                    "\x02B{id},{};o{},{};f{f_dir};c14;h{};w{};DQR;{data_part};\x03",
                    el.name,
                    el.x,
                    el.y,
                    el.barcode_height.max(1),
                    el.barcode_module.max(2)
                )
            } else {
                format!(
                    "\x02B{id},{};o{},{};f{f_dir};c0;h{};w{};D0,2;{data_part};\x03",
                    el.name,
                    el.x,
                    el.y,
                    el.barcode_height.max(30),
                    el.barcode_module.max(1)
                )
            }
        }
        ElementKind::Line => {
            format!(
                "\x02L{id};o{},{};f{f_dir};l{};w{};\x03",
                el.x,
                el.y,
                el.line_length.max(1),
                el.line_width.max(1)
            )
        }
        ElementKind::Box => {
            format!(
                "\x02W{id};o{},{};f{f_dir};l{};h{};w{};\x03",
                el.x,
                el.y,
                el.line_length.max(1),
                el.h_mult.max(1),
                el.line_width.max(1)
            )
        }
        ElementKind::Graphic => {
            let gid = el.graphic_id.unwrap_or(1);
            format!(
                "\x02U{id},{};o{},{};f{f_dir};c{gid};h{};w{};\x03",
                el.name,
                el.x,
                el.y,
                el.h_mult.max(1),
                el.w_mult.max(1)
            )
        }
    }
}

fn is_layout_record_line(line: &str) -> bool {
    let t = line.trim();
    t.starts_with("\x02H")
        || t.starts_with("\x02B")
        || t.starts_with("\x02L")
        || t.starts_with("\x02U")
        || t.starts_with("\x02W")
}

fn serialize_layout_lines(doc: &LabelDocument) -> Vec<String> {
    let deleted: HashSet<usize> = doc.deleted_source_lines.iter().copied().collect();
    let mut lines = doc.raw_layout_lines.clone();
    for el in &doc.elements {
        if el.raw_id == "L99" {
            continue;
        }
        if let Some(idx) = el.source_line {
            if !deleted.contains(&idx) && idx < lines.len() {
                lines[idx] = patch_layout_line(&lines[idx], el);
            }
        }
    }

    let mut out_lines: Vec<String> = lines
        .into_iter()
        .enumerate()
        .filter(|(idx, _)| !deleted.contains(idx))
        .map(|(_, l)| l)
        .collect();

    // Emit new graphic G+u blocks for any new_graphic_ids still referenced by a U element
    let mut new_graphic_lines = Vec::new();
    for gid in &doc.new_graphic_ids {
        let still_used = doc
            .elements
            .iter()
            .any(|el| el.kind == ElementKind::Graphic && el.graphic_id == Some(*gid));
        if still_used {
            if let Some(bmp) = doc.graphics.get(gid) {
                new_graphic_lines.extend(encode_ipl_6bit_graphic(*gid, bmp));
            }
        }
    }
    if !new_graphic_lines.is_empty() {
        let g_insert_pos = out_lines
            .iter()
            .position(|l| is_layout_record_line(l))
            .or_else(|| {
                out_lines
                    .iter()
                    .position(|l| l.contains("\x1BP"))
                    .map(|i| i + 1)
            })
            .unwrap_or(out_lines.len());
        out_lines.splice(g_insert_pos..g_insert_pos, new_graphic_lines);
    }

    // Emit new elements (source_line.is_none()) before <STX>R<ETX> or L99
    let new_element_lines: Vec<String> = doc
        .elements
        .iter()
        .filter(|el| el.raw_id != "L99" && el.source_line.is_none())
        .map(format_new_element_line)
        .collect();
    if !new_element_lines.is_empty() {
        let el_insert_pos = out_lines
            .iter()
            .position(|l| {
                let t = l.trim();
                t == "\x02R\x03" || t.starts_with("\x02L99;")
            })
            .unwrap_or(out_lines.len());
        out_lines.splice(el_insert_pos..el_insert_pos, new_element_lines);
    }

    out_lines
}

fn format_output(lines: Vec<String>, uses_tag_syntax: bool) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let out_lines: Vec<String> = if uses_tag_syntax {
        lines.iter().map(|l| restore_ascii_tags(l)).collect()
    } else {
        lines
    };
    let mut out = out_lines.join("\n");
    out.push('\n');
    out
}

/// Serialize only the layout section (`.TAX` format), preserving all untouched header/footer,
/// graphic (`G`/`u`), `L99`, and `d0,LEN` dynamic field definitions.
pub fn serialize_tax(doc: &LabelDocument) -> String {
    let lines = serialize_layout_lines(doc);
    format_output(lines, doc.uses_tag_syntax)
}

/// Serialize both the layout section and the DAX data section (`.DAX` format), updating
/// `<ESC>F"FIELD"<LF>VALUE` records for modified `dax_value`s while preserving untouched
/// data lines and `<RS>`/`<US>`/`<ETB>` footers.
pub fn serialize_dax(doc: &LabelDocument) -> String {
    let mut lines = serialize_layout_lines(doc);

    let mut dax_map: HashMap<String, String> = HashMap::new();
    for el in &doc.elements {
        if let Some(ref val) = el.dax_value {
            dax_map.insert(normalize_key(&el.name), val.clone());
        }
    }

    let mut seen_keys: HashSet<String> = HashSet::new();
    let mut data_lines = doc.raw_data_lines.clone();
    for line in &mut data_lines {
        let trimmed = line.trim();
        if let Some(caps) = DAX_DATA_RE.captures(trimmed) {
            let field_name = &caps[1];
            let orig_val = &caps[2];
            let key = normalize_key(field_name);
            seen_keys.insert(key.clone());
            if let Some(new_val) = dax_map.get(&key) {
                let suffix = if orig_val.ends_with(';') && !new_val.ends_with(';') {
                    ";"
                } else {
                    ""
                };
                *line = format!("\x02\x1BF\"{field_name}\"\x0A{new_val}{suffix}\x03");
            }
        }
    }

    let mut new_data_lines = Vec::new();
    for el in &doc.elements {
        if !el.is_static_text {
            if let Some(ref val) = el.dax_value {
                let key = normalize_key(&el.name);
                if seen_keys.insert(key) {
                    new_data_lines.push(format!("\x02\x1BF\"{}\"\x0A{val}\x03", el.name));
                }
            }
        }
    }
    if !new_data_lines.is_empty() {
        let insert_pos = data_lines
            .iter()
            .position(|l| l.contains('\x1E') || l.contains('\x17'))
            .unwrap_or(data_lines.len());
        data_lines.splice(insert_pos..insert_pos, new_data_lines);
    }

    lines.extend(data_lines);
    format_output(lines, doc.uses_tag_syntax)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_label_content;

    const SAMPLE_TAG_DAX: &str = "\
<STX>G1,0002;x2;y12;<ETX>
<STX>u0,@@;<ETX>
<STX>u1,O ;<ETX>
<STX>U64,0001;o10,20;f0;c1;h1;w1;<ETX>
<STX>H1,TITLE;o100,200;f0;h1;w1;c21;d3,OLD TITLE;<ETX>
<STX>H12,WIDTH ;o50,60;f1;h2;w2;c20;b0;d0,8;<ETX>
<STX>B5,CODE;o30,300;f0;c0;h80;w2;D0,2;d0,12;<ETX>
<STX>L1;o10,400;f0;l500;w3;<ETX>
<STX>L99;o0,0;f0;l100;w1;<ETX>
<STX>R<ETX>
<STX><ESC>E1,1<CAN><ETX>
<STX><ESC>F\"WIDTH \"<LF>1250<ETX>
<STX><ESC>F\"CODE\"<LF>ABC123<ETX>
<RS><US><ETB>
";

    #[test]
    fn round_trip_tax_edits_layout_and_preserves_d0_and_graphics() {
        let mut doc = parse_label_content(SAMPLE_TAG_DAX);

        // Edit static text element H1
        let h1 = doc.elements.iter_mut().find(|e| e.raw_id == "H1").unwrap();
        h1.x = 150;
        h1.y = 250;
        h1.h_mult = 3;
        h1.w_mult = 2;
        h1.weight_code = 22;
        h1.rotation = 90;
        h1.static_text = Some("NEW TITLE".to_string());

        // Edit dynamic element H12 (should NOT overwrite d0,8 in TAX)
        let h12 = doc.elements.iter_mut().find(|e| e.raw_id == "H12").unwrap();
        h12.x = 75;
        h12.y = 85;
        h12.rotation = 180;

        // Edit line L1
        let l1 = doc.elements.iter_mut().find(|e| e.raw_id == "L1").unwrap();
        l1.line_length = 750;
        l1.line_width = 5;

        let tax_out = serialize_tax(&doc);

        // Graphics, L99, and R are preserved verbatim
        assert!(tax_out.contains("<STX>G1,0002;x2;y12;<ETX>"));
        assert!(tax_out.contains("<STX>u0,@@;<ETX>"));
        assert!(tax_out.contains("<STX>u1,O ;<ETX>"));
        assert!(tax_out.contains("<STX>L99;o0,0;f0;l100;w1;<ETX>"));
        assert!(tax_out.contains("<STX>R<ETX>"));
        // TAX must NOT contain DAX data section
        assert!(!tax_out.contains("<ESC>F\"WIDTH \""));
        // Dynamic field d0,8 must remain untouched
        assert!(tax_out.contains("<STX>H12,WIDTH ;o75,85;f2;h2;w2;c20;b0;d0,8;<ETX>"));

        let reparsed = parse_label_content(&tax_out);
        let h1_re = reparsed.elements.iter().find(|e| e.raw_id == "H1").unwrap();
        assert_eq!(h1_re.x, 150);
        assert_eq!(h1_re.y, 250);
        assert_eq!(h1_re.h_mult, 3);
        assert_eq!(h1_re.w_mult, 2);
        assert_eq!(h1_re.weight_code, 22);
        assert_eq!(h1_re.rotation, 90);
        assert_eq!(h1_re.static_text.as_deref(), Some("NEW TITLE"));

        let l1_re = reparsed.elements.iter().find(|e| e.raw_id == "L1").unwrap();
        assert_eq!(l1_re.line_length, 750);
        assert_eq!(l1_re.line_width, 5);
    }

    #[test]
    fn round_trip_dax_edits_both_layout_and_data_values() {
        let mut doc = parse_label_content(SAMPLE_TAG_DAX);

        let h12 = doc.elements.iter_mut().find(|e| e.raw_id == "H12").unwrap();
        h12.x = 55;
        h12.dax_value = Some("2400".to_string());

        let b5 = doc.elements.iter_mut().find(|e| e.raw_id == "B5").unwrap();
        b5.barcode_height = 100;
        b5.barcode_module = 3;
        b5.dax_value = Some("XYZ789".to_string());

        let dax_out = serialize_dax(&doc);

        assert!(dax_out.contains("<STX><ESC>F\"WIDTH \"<LF>2400<ETX>"));
        assert!(dax_out.contains("<STX><ESC>F\"CODE\"<LF>XYZ789<ETX>"));
        assert!(dax_out.contains("<RS><US><ETB>"));
        assert!(dax_out.contains("<STX>G1,0002;x2;y12;<ETX>"));

        let reparsed = parse_label_content(&dax_out);
        let h12_re = reparsed.elements.iter().find(|e| e.raw_id == "H12").unwrap();
        assert_eq!(h12_re.x, 55);
        assert_eq!(h12_re.dax_value.as_deref(), Some("2400"));

        let b5_re = reparsed.elements.iter().find(|e| e.raw_id == "B5").unwrap();
        assert_eq!(b5_re.barcode_height, 100);
        assert_eq!(b5_re.barcode_module, 3);
        assert_eq!(b5_re.dax_value.as_deref(), Some("XYZ789"));
    }

    #[test]
    fn preserves_raw_control_bytes_when_input_is_binary() {
        let raw_binary = "\x02H1,MSG;o10,20;f0;h1;w1;c20;d3,HI;\x03\n\x02\x1BF\"X\"\x0A99\x03\n";
        let mut doc = parse_label_content(raw_binary);
        assert!(!doc.uses_tag_syntax);

        let h1 = doc.elements.iter_mut().find(|e| e.raw_id == "H1").unwrap();
        h1.static_text = Some("BYE".to_string());

        let out = serialize_dax(&doc);
        assert!(!out.contains("<STX>"));
        assert!(out.contains("\x02H1,MSG;o10,20;f0;h1;w1;c20;d3,BYE;\x03"));
        assert!(out.contains("\x02\x1BF\"X\"\x0A99\x03"));
    }

    #[test]
    fn element_bounds_accounts_for_rotations() {
        let doc = parse_label_content(SAMPLE_TAG_DAX);
        let u64_el = doc.elements.iter().find(|e| e.raw_id == "U64").unwrap();
        // Graphic is 2x12 at (10, 20), f0 (rotation 0)
        assert_eq!(doc.element_bounds(u64_el), (10.0, 20.0, 2.0, 12.0));

        let mut rotated = u64_el.clone();
        rotated.rotation = -90;
        assert_eq!(doc.element_bounds(&rotated), (10.0, 18.0, 12.0, 2.0));

        rotated.rotation = 90;
        assert_eq!(doc.element_bounds(&rotated), (-2.0, 20.0, 12.0, 2.0));

        rotated.rotation = 180;
        assert_eq!(doc.element_bounds(&rotated), (8.0, 8.0, 2.0, 12.0));
    }

    #[test]
    fn box_w_round_trip() {
        let input = "\
<STX>W2;o100,150;f0;l300;h120;w4;<ETX>
<STX>R<ETX>
";
        let mut doc = parse_label_content(input);
        let box_el = doc.elements.iter_mut().find(|e| e.raw_id == "W2").unwrap();
        assert_eq!(box_el.kind, ElementKind::Box);
        assert_eq!(box_el.line_length, 300);
        assert_eq!(box_el.h_mult, 120);
        assert_eq!(box_el.line_width, 4);

        box_el.x = 110;
        box_el.line_length = 450;
        box_el.h_mult = 200;
        box_el.line_width = 6;

        let out = serialize_tax(&doc);
        assert!(out.contains("<STX>W2;o110,150;f0;l450;h200;w6;<ETX>"));

        let reparsed = parse_label_content(&out);
        let re_box = reparsed.elements.iter().find(|e| e.raw_id == "W2").unwrap();
        assert_eq!(re_box.kind, ElementKind::Box);
        assert_eq!(re_box.x, 110);
        assert_eq!(re_box.line_length, 450);
        assert_eq!(re_box.h_mult, 200);
        assert_eq!(re_box.line_width, 6);
    }

    #[test]
    fn qr_code_render_and_round_trip() {
        let input = "\
<STX>B7,QRCODE;o50,60;f0;c14;h100;w4;DQR;d3,HELLO-QR;<ETX>
<STX>R<ETX>
";
        let doc = parse_label_content(input);
        let qr_el = doc.elements.iter().find(|e| e.raw_id == "B7").unwrap();
        assert_eq!(qr_el.kind, ElementKind::Barcode);
        assert_eq!(qr_el.barcode_type.as_deref(), Some("QR"));
        assert_eq!(qr_el.static_text.as_deref(), Some("HELLO-QR"));

        let svg = crate::svg_renderer::render_to_svg(&doc, 400, 400, false);
        assert!(svg.contains(r#"data-tax-id="B7""#));
        assert!(svg.contains("<path d=\"M"));

        let out = serialize_tax(&doc);
        let reparsed = parse_label_content(&out);
        let re_qr = reparsed.elements.iter().find(|e| e.raw_id == "B7").unwrap();
        assert_eq!(re_qr.barcode_type.as_deref(), Some("QR"));
        assert_eq!(re_qr.static_text.as_deref(), Some("HELLO-QR"));
    }

    #[test]
    fn ipl_6bit_graphic_encode_decode_pixel_exact_round_trip() {
        let width = 5;
        let height = 14; // spans 3 6-bit chars (18 bits, last 4 unused)
        let mut pixels = vec![false; width * height];
        // Set a pattern including an all-ones 6-bit group (rows 0..6 in col 2 -> ' ')
        for r in 0..6 {
            pixels[r * width + 2] = true;
        }
        pixels[0 * width + 0] = true;
        pixels[13 * width + 4] = true;
        pixels[7 * width + 1] = true;
        pixels[8 * width + 3] = true;

        let gid = 3;
        let bmp = GraphicBitmap {
            id: gid,
            width,
            height,
            pixels: pixels.clone(),
        };

        let mut doc = LabelDocument {
            uses_tag_syntax: true,
            raw_layout_lines: vec!["\x02R\x03".to_string()],
            ..Default::default()
        };
        doc.graphics.insert(gid, bmp);
        doc.new_graphic_ids.push(gid);
        doc.elements.push(TaxElement {
            raw_id: "U10".to_string(),
            name: "LOGO".to_string(),
            kind: ElementKind::Graphic,
            x: 20,
            y: 30,
            font_id: None,
            h_mult: 1,
            w_mult: 1,
            static_text: None,
            is_static_text: false,
            dax_value: None,
            source_line: None,
            line_length: 0,
            line_width: 1,
            line_vertical: false,
            barcode_height: 0,
            barcode_module: 1,
            barcode_type: None,
            rotation: 0,
            weight_code: 1,
            point_size: None,
            graphic_id: Some(gid),
        });

        let tax_out = serialize_tax(&doc);
        assert!(tax_out.contains("<STX>G3,0002;x5;y14;<ETX>"));
        assert!(tax_out.contains("<STX>U10,LOGO;o20,30;f0;c3;h1;w1;<ETX>"));

        let reparsed = parse_label_content(&tax_out);
        let re_bmp = reparsed.graphics.get(&gid).expect("re-parsed graphic 3");
        assert_eq!(re_bmp.width, width);
        assert_eq!(re_bmp.height, height);
        assert_eq!(re_bmp.pixels, pixels);
    }

    #[test]
    fn add_and_delete_element_round_trip() {
        let mut doc = parse_label_content(SAMPLE_TAG_DAX);

        // Delete H1 (source_line)
        let h1_idx = doc
            .elements
            .iter()
            .position(|e| e.raw_id == "H1")
            .unwrap();
        if let Some(sl) = doc.elements[h1_idx].source_line {
            doc.deleted_source_lines.push(sl);
        }
        doc.elements.remove(h1_idx);

        // Add a new dynamic Text element and a new Box element
        let new_id_1 = doc.next_available_field_id();
        doc.elements.push(TaxElement {
            raw_id: format!("H{new_id_1}"),
            name: "NEWDYN".to_string(),
            kind: ElementKind::Text,
            x: 120,
            y: 220,
            font_id: None,
            h_mult: 2,
            w_mult: 2,
            static_text: None,
            is_static_text: false,
            dax_value: Some("HELLO_DAX".to_string()),
            source_line: None,
            line_length: 0,
            line_width: 1,
            line_vertical: false,
            barcode_height: 0,
            barcode_module: 1,
            barcode_type: None,
            rotation: 0,
            weight_code: 20,
            point_size: None,
            graphic_id: None,
        });

        let new_id_2 = doc.next_available_field_id();
        doc.elements.push(TaxElement {
            raw_id: format!("W{new_id_2}"),
            name: format!("W{new_id_2}"),
            kind: ElementKind::Box,
            x: 15,
            y: 25,
            font_id: None,
            h_mult: 80,
            w_mult: 1,
            static_text: None,
            is_static_text: false,
            dax_value: None,
            source_line: None,
            line_length: 160,
            line_width: 3,
            line_vertical: false,
            barcode_height: 0,
            barcode_module: 1,
            barcode_type: None,
            rotation: 0,
            weight_code: 1,
            point_size: None,
            graphic_id: None,
        });

        let dax_out = serialize_dax(&doc);
        assert!(!dax_out.contains("H1,TITLE"));
        assert!(dax_out.contains(&format!("<STX>H{new_id_1},NEWDYN;o120,220;")));
        assert!(dax_out.contains(&format!("<STX>W{new_id_2};o15,25;f0;l160;h80;w3;<ETX>")));
        assert!(dax_out.contains("<STX><ESC>F\"NEWDYN\"<LF>HELLO_DAX<ETX>"));

        let reparsed = parse_label_content(&dax_out);
        assert!(reparsed.elements.iter().all(|e| e.raw_id != "H1"));
        let re_dyn = reparsed
            .elements
            .iter()
            .find(|e| e.raw_id == format!("H{new_id_1}"))
            .unwrap();
        assert_eq!(re_dyn.dax_value.as_deref(), Some("HELLO_DAX"));
        let re_box = reparsed
            .elements
            .iter()
            .find(|e| e.raw_id == format!("W{new_id_2}"))
            .unwrap();
        assert_eq!(re_box.kind, ElementKind::Box);
        assert_eq!(re_box.line_length, 160);
        assert_eq!(re_box.h_mult, 80);
        assert_eq!(re_box.line_width, 3);
    }
}
