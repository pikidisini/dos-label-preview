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
}
