use std::collections::HashMap;
use once_cell::sync::Lazy;
use regex::Regex;

use crate::model::{ElementKind, GraphicBitmap, LabelDocument, TaxElement};

static RECORD_LINE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\x02([HBU])(\d+),([^;]+);(.*)\x03").unwrap());
static LINE_RECORD_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\x02L(\d+);(.*)\x03").unwrap());
static BOX_RECORD_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\x02W(\d+)(?:,([^;]+))?;(.*)\x03").unwrap());
static GRAPHIC_HEADER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\x02G(\d+),[^;]*;(.*)\x03").unwrap());
static GRAPHIC_ROW_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\x02u(\d+),([^;\x03]*);?\x03").unwrap());

static ORIGIN_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"o(\d+),(\d+)").unwrap());
static FONT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"f(\d+)").unwrap());
static HEIGHT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"h(\d+)").unwrap());
static WIDTH_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"w(\d+)").unwrap());
static X_DIM_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"x(\d+)").unwrap());
static Y_DIM_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"y(\d+)").unwrap());
static DATA_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"d(\d+),(.*)").unwrap());
static LINE_LEN_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"l(\d+)").unwrap());
static POINT_SIZE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"k(\d+)").unwrap());
static WEIGHT_OR_GRAPHIC_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"c(\d+)").unwrap());
static DAX_DATA_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"\x02\x1BF"([^"]+)"\x0A([^\x03]*)\x03"#).unwrap());
static NON_ALNUM_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[^A-Z0-9]").unwrap());

/// Replace human-readable ASCII control tags (`<STX>`, `<ETX>`, etc.) with raw control bytes.
pub fn replace_ascii_tags(input: &str) -> String {
    input
        .replace("<STX>", "\x02")
        .replace("<ETX>", "\x03")
        .replace("<ESC>", "\x1B")
        .replace("<LF>", "\x0A")
        .replace("<RS>", "\x1E")
        .replace("<US>", "\x1F")
        .replace("<ETB>", "\x17")
        .replace("<CAN>", "\x18")
}

pub(crate) fn normalize_key(key: &str) -> String {
    NON_ALNUM_RE
        .replace_all(&key.trim().to_uppercase(), "")
        .into_owned()
}

/// Split the DAX data section into logical records/lines without breaking on `\x0A` (`<LF>`)
/// inside `\x02...\x03` (`<STX>...<ETX>`) blocks.
fn split_data_records(data_section: &str) -> Vec<String> {
    if data_section.is_empty() {
        return Vec::new();
    }
    let mut records = Vec::new();
    let mut current = String::new();
    let mut in_stx = false;

    for ch in data_section.chars() {
        match ch {
            '\x02' => {
                in_stx = true;
                current.push(ch);
            }
            '\x03' => {
                in_stx = false;
                current.push(ch);
            }
            '\n' if !in_stx => {
                let trimmed = current.trim_end_matches('\r');
                if !trimmed.is_empty() {
                    records.push(trimmed.to_string());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    let trimmed = current.trim_end_matches('\r');
    if !trimmed.is_empty() {
        records.push(trimmed.to_string());
    }
    records
}

fn decode_ipl_6bit_char(byte: u8) -> u8 {
    // Advanced mode 6-bit encoding: 0x40 ('@')..=0x7E ('~') -> 0..=62,
    // 0x7F (DEL) or 0x20 (' ', common printable substitute) -> 63 (0b11_1111).
    if byte == b' ' || byte == 0x7F {
        0b11_1111
    } else if (0x40..=0x7E).contains(&byte) {
        (byte - 0x40) & 0x3F
    } else {
        0
    }
}

/// Write one vertical column (`u<col>`) into a row-major `GraphicBitmap`.
/// Per IPL Developer's Guide (Ch. 3, "Creating User-Defined Characters for Advanced Mode"):
/// - Each `uN` command defines column `N` from top (`row 0`) to bottom (`row height-1`).
/// - Within each 6-bit character, bit 0 (LSB) is the top pixel and bit 5 is the bottom pixel.
fn write_ipl_column(bmp: &mut GraphicBitmap, col_idx: usize, data: &str) {
    if col_idx >= bmp.width || bmp.height == 0 {
        return;
    }
    for (char_idx, byte) in data.bytes().enumerate() {
        let bits = decode_ipl_6bit_char(byte);
        for bit in 0..6 {
            let row = char_idx * 6 + bit;
            if row >= bmp.height {
                break;
            }
            if (bits & (1 << bit)) != 0 {
                bmp.pixels[row * bmp.width + col_idx] = true;
            }
        }
    }
}

fn parse_capture_i32(re: &Regex, text: &str, group: usize) -> Option<i32> {
    re.captures(text)
        .and_then(|c| c.get(group))
        .and_then(|m| m.as_str().parse::<i32>().ok())
}

fn parse_capture_str<'a>(re: &Regex, text: &'a str, group: usize) -> Option<&'a str> {
    re.captures(text)
        .and_then(|c| c.get(group))
        .map(|m| m.as_str())
}

/// Parse a `.TAX` or `.DAX` file content into a `LabelDocument`.
pub fn parse_label_content(raw_content: &str) -> LabelDocument {
    let uses_tag_syntax = raw_content.contains("<STX>") || raw_content.contains("<ETX>");
    let normalized = replace_ascii_tags(raw_content);
    let data_marker = "\x02\x1BF";
    let (layout_section, data_section) = match normalized.find(data_marker) {
        Some(idx) => (&normalized[..idx], &normalized[idx..]),
        None => (normalized.as_str(), ""),
    };

    let mut doc = LabelDocument {
        uses_tag_syntax,
        raw_layout_lines: layout_section.lines().map(|l| l.to_string()).collect(),
        raw_data_lines: split_data_records(data_section),
        ..Default::default()
    };
    let mut active_graphic_id: Option<u32> = None;

    for (line_idx, raw_line) in layout_section.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        // 1. Check for IPL User-Defined Graphic header: <STX>G1,0002;x240;y54;<ETX>
        // Per IPL Developer's Guide: x = width (columns), y = height (rows in dots).
        if let Some(caps) = GRAPHIC_HEADER_RE.captures(line) {
            if let Ok(gid) = caps[1].parse::<u32>() {
                let attrs = &caps[2];
                let width = parse_capture_i32(&X_DIM_RE, attrs, 1).unwrap_or(0).max(0) as usize;
                let height = parse_capture_i32(&Y_DIM_RE, attrs, 1).unwrap_or(0).max(0) as usize;
                doc.graphics.insert(
                    gid,
                    GraphicBitmap {
                        id: gid,
                        width,
                        height,
                        pixels: vec![false; width * height],
                    },
                );
                active_graphic_id = Some(gid);
            }
            continue;
        }

        // 2. Check for IPL User-Defined Graphic column: <STX>u0,@@@@@@@@@;<ETX>
        if let Some(caps) = GRAPHIC_ROW_RE.captures(line) {
            if let (Some(gid), Ok(col_idx)) = (active_graphic_id, caps[1].parse::<usize>()) {
                if let Some(bmp) = doc.graphics.get_mut(&gid) {
                    let col_data = &caps[2];
                    if bmp.height == 0 && !col_data.is_empty() {
                        bmp.height = col_data.len() * 6;
                    }
                    if col_idx >= bmp.width {
                        let new_w = col_idx + 1;
                        let mut new_pixels = vec![false; new_w * bmp.height];
                        for r in 0..bmp.height {
                            let old_start = r * bmp.width;
                            let new_start = r * new_w;
                            new_pixels[new_start..new_start + bmp.width]
                                .copy_from_slice(&bmp.pixels[old_start..old_start + bmp.width]);
                        }
                        bmp.width = new_w;
                        bmp.pixels = new_pixels;
                    }
                    write_ipl_column(bmp, col_idx, col_data);
                }
            }
            continue;
        }

        // 3. Parse H (Text), B (Barcode), U (User-defined graphic placement), L (Line), or W (Box)
        let (rec_type, rec_id, name, attrs_str) = if let Some(caps) = RECORD_LINE_RE.captures(line) {
            (
                caps[1].chars().next().unwrap(),
                caps[2].to_string(),
                caps[3].trim().to_string(),
                caps[4].to_string(),
            )
        } else if let Some(caps) = LINE_RECORD_RE.captures(line) {
            let id = caps[1].to_string();
            (
                'L',
                id.clone(),
                format!("L{id}"),
                caps[2].to_string(),
            )
        } else if let Some(caps) = BOX_RECORD_RE.captures(line) {
            let id = caps[1].to_string();
            let name = caps
                .get(2)
                .map(|m| m.as_str().trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("W{id}"));
            ('W', id, name, caps[3].to_string())
        } else {
            continue;
        };

        let (ox, oy) = ORIGIN_RE
            .captures(&attrs_str)
            .map(|c| {
                (
                    c[1].parse::<i32>().unwrap_or(0),
                    c[2].parse::<i32>().unwrap_or(0),
                )
            })
            .unwrap_or((0, 0));

        let font_id = parse_capture_str(&FONT_RE, &attrs_str, 1).map(|s| s.to_string());
        let h_mult = parse_capture_i32(&HEIGHT_RE, &attrs_str, 1).unwrap_or(1);
        let w_mult = parse_capture_i32(&WIDTH_RE, &attrs_str, 1).unwrap_or(1);
        let c_val = parse_capture_i32(&WEIGHT_OR_GRAPHIC_RE, &attrs_str, 1).unwrap_or(20);

        let (static_text, is_static_text) = if let Some(dcaps) = DATA_RE.captures(&attrs_str) {
            if &dcaps[1] == "3" {
                let text = dcaps[2].trim_end_matches(';').trim().to_string();
                (Some(text), true)
            } else {
                (None, false)
            }
        } else {
            (None, false)
        };

        // Field Direction `fN`: rotates counterclockwise in IPL.
        // SVG `rotate(deg)` is clockwise, so:
        // f0 -> 0°, f1 (90° CCW) -> -90°, f2 (180°) -> 180°, f3 (270° CCW) -> +90°.
        let rotation = match font_id.as_deref() {
            Some("1") => -90,
            Some("2") => 180,
            Some("3") => 90,
            _ => 0,
        };
        let point_size = parse_capture_i32(&POINT_SIZE_RE, &attrs_str, 1);

        let kind = match rec_type {
            'B' => ElementKind::Barcode,
            'L' => ElementKind::Line,
            'W' => ElementKind::Box,
            'U' => ElementKind::Graphic,
            _ => ElementKind::Text,
        };

        let line_length = if matches!(kind, ElementKind::Line | ElementKind::Box) {
            parse_capture_i32(&LINE_LEN_RE, &attrs_str, 1).unwrap_or(0)
        } else {
            0
        };
        let line_width = if matches!(kind, ElementKind::Line | ElementKind::Box) {
            w_mult.max(1)
        } else {
            1
        };
        let line_vertical = kind == ElementKind::Line
            && matches!(font_id.as_deref(), Some("1") | Some("3"));

        let barcode_height = if kind == ElementKind::Barcode {
            h_mult
        } else {
            0
        };
        let barcode_module = if kind == ElementKind::Barcode {
            w_mult.max(1)
        } else {
            1
        };
        let barcode_type = if kind == ElementKind::Barcode {
            if attrs_str.contains("DQR") || c_val == 14 || attrs_str.contains("c7,") {
                Some("QR".to_string())
            } else {
                Some("CODE39".to_string())
            }
        } else {
            None
        };

        let graphic_id = if kind == ElementKind::Graphic {
            parse_capture_i32(&WEIGHT_OR_GRAPHIC_RE, &attrs_str, 1)
                .filter(|&v| v > 0)
                .map(|v| v as u32)
        } else {
            None
        };

        doc.elements.push(TaxElement {
            raw_id: format!("{rec_type}{rec_id}"),
            name,
            kind,
            x: ox,
            y: oy,
            font_id,
            h_mult,
            w_mult,
            static_text,
            is_static_text,
            dax_value: None,
            source_line: Some(line_idx),
            line_length,
            line_width,
            line_vertical,
            barcode_height,
            barcode_module,
            barcode_type,
            rotation,
            weight_code: c_val,
            point_size,
            graphic_id,
        });
    }

    // 4. Inject DAX data values into dynamic elements
    if !data_section.is_empty() {
        let mut dax_map: HashMap<String, String> = HashMap::new();
        for caps in DAX_DATA_RE.captures_iter(data_section) {
            let key = normalize_key(&caps[1]);
            let value = caps[2].trim_end_matches(';').to_string();
            dax_map.insert(key, value);
        }

        for el in &mut doc.elements {
            let key = normalize_key(&el.name);
            if let Some(val) = dax_map.get(&key) {
                el.dax_value = Some(val.clone());
            }
        }
    }

    // 5. Estimate bounding box for auto-sizing, ignoring printer-hidden/shifted elements
    // (both huge coordinates like 9999 and separated off-label clusters like y=999).
    const SHIFT_HIDE_THRESHOLD_X: i32 = 4000;
    const SHIFT_HIDE_THRESHOLD_Y: i32 = 3000;

    let mut boxes: Vec<(f32, f32, f32, f32)> = Vec::new();
    for el in &doc.elements {
        if el.raw_id == "L99"
            || el.x < 0
            || el.y < 0
            || el.x >= SHIFT_HIDE_THRESHOLD_X
            || el.y >= SHIFT_HIDE_THRESHOLD_Y
        {
            continue;
        }
        boxes.push(doc.element_bounds(el));
    }

    // Sort by top Y to detect a large vertical gap separating main label from shifted-off elements.
    boxes.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut cluster_max_y: f32 = 0.0;
    let mut max_x: i32 = 0;
    for (i, (bx, by, bw, bh)) in boxes.iter().enumerate() {
        if i > 0 && (*by - cluster_max_y) > 300.0 && *by > (cluster_max_y * 2.0) {
            break;
        }
        max_x = max_x.max((*bx + *bw).ceil() as i32);
        cluster_max_y = cluster_max_y.max(*by + *bh);
    }

    doc.detected_width = max_x;
    doc.detected_height = cluster_max_y.ceil() as i32;

    doc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_ipl_6bit_graphic_and_dax_injection() {
        // x2 = 2 columns (u0, u1), y12 = 12 rows (2 chars per column).
        let sample = "\
<STX>G1,0002;x2;y12;<ETX>
<STX>u0,@@;<ETX>
<STX>u1,O ;<ETX>
<STX>U64,0001;o10,20;f0;c1;h1;w1;<ETX>
<STX>H12,WIDTH ;o50,60;f1;h2;w2;c20;b0;d0,8;<ETX>
<STX>R<ETX>
<STX><ESC>E1,1<CAN><ETX>
<STX><ESC>F\"WIDTH \"<LF>1250<ETX>
";
        let doc = parse_label_content(sample);
        let bmp = doc.graphics.get(&1).expect("graphic 1 should exist");
        assert_eq!(bmp.width, 2);
        assert_eq!(bmp.height, 12);
        // Column 0 ("@@") -> all false; Column 1 ("O ") -> 'O' (bits 0..3 true), ' ' (bits 0..5 true)
        for row in 0..12 {
            assert!(!bmp.pixels[row * 2], "col 0 row {row} should be false");
        }
        assert_eq!(
            (0..6).map(|r| bmp.pixels[r * 2 + 1]).collect::<Vec<_>>(),
            vec![true, true, true, true, false, false]
        );
        assert_eq!(
            (6..12).map(|r| bmp.pixels[r * 2 + 1]).collect::<Vec<_>>(),
            vec![true, true, true, true, true, true]
        );

        let text_el = doc
            .elements
            .iter()
            .find(|e| e.raw_id == "H12")
            .expect("H12 should exist");
        assert!(doc.uses_tag_syntax);
        assert_eq!(text_el.static_text, None);
        assert_eq!(text_el.dax_value.as_deref(), Some("1250"));
        assert_eq!(text_el.display_text(), Some("1250"));
        assert_eq!(text_el.rotation, -90);
        assert!(text_el.source_line.is_some());
    }

    #[test]
    fn auto_size_ignores_shifted_9999_and_999_gap_elements() {
        let sample = "\
<STX><ESC>P;<ETX>
<STX>H1,TITLE ;o40,50;f0;h2;w2;c20;d3,VISIBLE;<ETX>
<STX>H2,HIDDEN;o40,999;f0;h1;w1;c20;d3,SHIFTED 999;<ETX>
<STX>H3,HIDE4 ;o40,9999;f0;h1;w1;c20;d3,SHIFTED 9999;<ETX>
<STX>H4,HIDEX ;o9999,100;f0;h1;w1;c20;d3,SHIFTED X;<ETX>
<STX>R<ETX>
";
        let doc = parse_label_content(sample);
        assert!(doc.detected_width < 1000, "width was {}", doc.detected_width);
        assert!(doc.detected_height < 200, "height was {}", doc.detected_height);
        assert_eq!(doc.elements.len(), 4);
    }
}
