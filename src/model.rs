use std::collections::HashMap;

/// Element types parsed from an Intermec IPL / TAX / DAX label file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElementKind {
    Text,
    Barcode,
    Line,
    Graphic,
}

/// A single layout element on the label canvas.
#[derive(Debug, Clone)]
pub struct TaxElement {
    pub raw_id: String,
    pub name: String,
    pub kind: ElementKind,
    pub x: i32,
    pub y: i32,
    pub font_id: Option<String>,
    pub h_mult: i32,
    pub w_mult: i32,
    pub static_text: Option<String>,
    pub is_static_text: bool,
    pub dax_value: Option<String>,
    pub source_line: Option<usize>,
    pub line_length: i32,
    pub line_width: i32,
    pub line_vertical: bool,
    pub barcode_height: i32,
    pub barcode_module: i32,
    pub barcode_type: Option<String>,
    pub rotation: i32,
    pub weight_code: i32,
    pub point_size: Option<i32>,
    /// Graphic slot ID referenced by `U` records (e.g. `c1` -> `1`).
    pub graphic_id: Option<u32>,
}

impl TaxElement {
    /// Returns the effective display text: `static_text` for static `d3` fields,
    /// or `dax_value` for dynamic `d0` fields injected from DAX data.
    pub fn display_text(&self) -> Option<&str> {
        if self.is_static_text {
            self.static_text.as_deref()
        } else {
            self.dax_value.as_deref()
        }
    }
}

/// Decoded 1-bit-per-pixel printer graphic (`G<id>,...;x<H>;y<W>;` + `u<row>,<data>;`).
#[derive(Debug, Clone)]
pub struct GraphicBitmap {
    pub id: u32,
    pub width: usize,
    pub height: usize,
    /// Row-major 1-bit pixel buffer (`true` = black/ink dot).
    pub pixels: Vec<bool>,
}

/// Complete parsed label document containing layout elements and embedded graphics.
#[derive(Debug, Clone, Default)]
pub struct LabelDocument {
    pub elements: Vec<TaxElement>,
    pub graphics: HashMap<u32, GraphicBitmap>,
    pub detected_width: i32,
    pub detected_height: i32,
    pub raw_layout_lines: Vec<String>,
    pub raw_data_lines: Vec<String>,
    pub uses_tag_syntax: bool,
}

impl LabelDocument {
    /// Compute the axis-aligned bounding box `(x, y, width, height)` in label dots
    /// for hit-testing and selection highlighting, accounting for `rotation` (`0`, `-90`, `180`, `90`).
    pub fn element_bounds(&self, el: &TaxElement) -> (f32, f32, f32, f32) {
        let ex = el.x as f32;
        let ey = el.y as f32;

        if el.kind == ElementKind::Line {
            let len = el.line_length.max(1) as f32;
            let stroke = el.line_width.max(1) as f32;
            let half = stroke * 0.5;
            let (x2, y2) = match el.rotation {
                -90 => (ex, ey - len),
                180 => (ex - len, ey),
                90 => (ex, ey + len),
                _ => {
                    if el.line_vertical {
                        (ex, ey + len)
                    } else {
                        (ex + len, ey)
                    }
                }
            };
            let min_x = ex.min(x2) - half;
            let min_y = ey.min(y2) - half;
            let w = (ex - x2).abs() + stroke;
            let h = (ey - y2).abs() + stroke;
            return (min_x, min_y, w, h);
        }

        let (local_w, local_h) = match el.kind {
            ElementKind::Graphic => {
                if let Some(bmp) = el.graphic_id.and_then(|id| self.graphics.get(&id)) {
                    (
                        (bmp.width as f32) * (el.w_mult.max(1) as f32),
                        (bmp.height as f32) * (el.h_mult.max(1) as f32),
                    )
                } else {
                    (0.0, 0.0)
                }
            }
            ElementKind::Barcode => {
                let text = el
                    .display_text()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&el.name);
                let char_count = text.chars().count().max(1) + 2; // *start* + *stop*
                let modules = (char_count * 13).saturating_sub(1) as f32;
                let w = modules * (el.barcode_module.max(1) as f32);
                let h = el.barcode_height.max(30) as f32;
                (w, h)
            }
            ElementKind::Text => {
                let text = el
                    .display_text()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&el.name);
                let base_size = if let Some(pt) = el.point_size {
                    (pt.max(4) as f32) * 203.2 / 72.0
                } else {
                    match el.weight_code {
                        2 => 20.0,
                        3 => 26.0,
                        20 => 24.0,
                        21 => 26.0,
                        22 => 32.0,
                        _ => 24.0,
                    }
                };
                let sx = el.w_mult.max(1) as f32;
                let sy = el.h_mult.max(1) as f32;
                let char_count = text.chars().count().max(1) as f32;
                (char_count * base_size * 0.6 * sx, base_size * sy)
            }
            ElementKind::Line => unreachable!(),
        };

        match el.rotation {
            -90 => (ex, ey - local_w, local_h, local_w),
            180 => (ex - local_w, ey - local_h, local_w, local_h),
            90 => (ex - local_h, ey, local_h, local_w),
            _ => (ex, ey, local_w, local_h),
        }
    }
}
