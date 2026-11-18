use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::Regex;

use crate::model::{ElementKind, LabelDocument, TaxElement};
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
                ElementKind::Line => el.line_width,
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
    if el.kind == ElementKind::Line {
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

fn serialize_layout_lines(doc: &LabelDocument) -> Vec<String> {
    let mut lines = doc.raw_layout_lines.clone();
    for el in &doc.elements {
        if el.raw_id == "L99" {
            continue;
        }
        if let Some(idx) = el.source_line {
            if idx < lines.len() {
                lines[idx] = patch_layout_line(&lines[idx], el);
            }
        }
    }
    lines
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

    let mut data_lines = doc.raw_data_lines.clone();
    for line in &mut data_lines {
        let trimmed = line.trim();
        if let Some(caps) = DAX_DATA_RE.captures(trimmed) {
            let field_name = &caps[1];
            let orig_val = &caps[2];
            let key = normalize_key(field_name);
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
}
