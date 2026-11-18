#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod model;
mod parser;
mod svg_renderer;
mod writer;

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use model::{ElementKind, GraphicBitmap, LabelDocument, TaxElement};
use svg_renderer::{DEFAULT_PAPER_HEIGHT, DEFAULT_PAPER_WIDTH};

const MAX_UNDO_HISTORY: usize = 100;
const HIT_TEST_PADDING_DOTS: f32 = 6.0;
const SELECTION_CYAN: egui::Color32 = egui::Color32::from_rgb(56, 189, 248);

fn main() -> eframe::Result<()> {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).ok();
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 800.0])
        .with_title("DOS Label Preview (TAX / DAX)");
    if let Some(icon_data) = icon {
        viewport = viewport.with_icon(Arc::new(icon_data));
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "DOS Label Preview",
        options,
        Box::new(|cc| Ok(Box::new(LabelPreviewApp::new(cc)))),
    )
}

#[derive(Debug, Clone)]
struct CanvasDragState {
    element_index: usize,
    start_x: i32,
    start_y: i32,
    accum_delta: egui::Vec2,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ElementFilter {
    All,
    Text,
    Barcode,
    Line,
    Box,
    Graphic,
}

impl ElementFilter {
    fn matches(self, kind: &ElementKind) -> bool {
        match self {
            Self::All => true,
            Self::Text => *kind == ElementKind::Text,
            Self::Barcode => *kind == ElementKind::Barcode,
            Self::Line => *kind == ElementKind::Line,
            Self::Box => *kind == ElementKind::Box,
            Self::Graphic => *kind == ElementKind::Graphic,
        }
    }
}

struct LabelPreviewApp {
    current_path: Option<PathBuf>,
    doc: Option<LabelDocument>,
    svg_cache: String,
    texture: Option<egui::TextureHandle>,
    fontdb_loaded: bool,
    usvg_options: resvg::usvg::Options<'static>,

    show_var_names: bool,
    auto_size: bool,
    canvas_width: i32,
    canvas_height: i32,

    zoom: f32,
    pan: egui::Vec2,
    fit_requested: bool,
    status_message: String,

    selected_index: Option<usize>,
    element_filter: String,
    element_kind_filter: ElementFilter,
    canvas_drag: Option<CanvasDragState>,
    inspector_undo_snapshot: Option<LabelDocument>,
    undo_stack: Vec<LabelDocument>,
    redo_stack: Vec<LabelDocument>,
}

impl LabelPreviewApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let mut usvg_options = resvg::usvg::Options::default();
        usvg_options.fontdb_mut().load_system_fonts();

        Self {
            current_path: None,
            doc: None,
            svg_cache: String::new(),
            texture: None,
            fontdb_loaded: true,
            usvg_options,
            show_var_names: false,
            auto_size: true,
            canvas_width: DEFAULT_PAPER_WIDTH,
            canvas_height: DEFAULT_PAPER_HEIGHT,
            zoom: 0.45,
            pan: egui::Vec2::ZERO,
            fit_requested: false,
            status_message: "Drop file .DAX atau .TAX ke jendela ini, atau klik 'Buka File'."
                .to_string(),
            selected_index: None,
            element_filter: String::new(),
            element_kind_filter: ElementFilter::All,
            canvas_drag: None,
            inspector_undo_snapshot: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    fn push_undo_snapshot(&mut self, snapshot: LabelDocument) {
        if self.undo_stack.len() >= MAX_UNDO_HISTORY {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(snapshot);
        self.redo_stack.clear();
    }

    fn undo(&mut self, ctx: &egui::Context) {
        let Some(prev) = self.undo_stack.pop() else {
            return;
        };
        if let Some(curr) = self.doc.replace(prev) {
            self.redo_stack.push(curr);
        }
        self.validate_selection();
        self.canvas_drag = None;
        self.inspector_undo_snapshot = None;
        self.rebuild_preview(ctx);
        self.status_message = "Undo diterapkan.".to_string();
    }

    fn redo(&mut self, ctx: &egui::Context) {
        let Some(next) = self.redo_stack.pop() else {
            return;
        };
        if let Some(curr) = self.doc.replace(next) {
            self.undo_stack.push(curr);
        }
        self.validate_selection();
        self.canvas_drag = None;
        self.inspector_undo_snapshot = None;
        self.rebuild_preview(ctx);
        self.status_message = "Redo diterapkan.".to_string();
    }

    fn validate_selection(&mut self) {
        if let (Some(idx), Some(doc)) = (self.selected_index, &self.doc) {
            if idx >= doc.elements.len() || doc.elements[idx].raw_id == "L99" {
                self.selected_index = None;
            }
        } else {
            self.selected_index = None;
        }
    }

    fn ensure_doc(&mut self) {
        if self.doc.is_none() {
            self.doc = Some(LabelDocument {
                uses_tag_syntax: true,
                raw_layout_lines: vec!["<STX><ESC>P;<ETX>".into(), "<STX>R<ETX>".into()],
                ..Default::default()
            });
            self.fit_requested = true;
        }
    }

    fn make_base_element(raw_id: String, name: String, kind: ElementKind) -> TaxElement {
        TaxElement {
            raw_id,
            name,
            kind,
            x: 40,
            y: 40,
            font_id: None,
            h_mult: 1,
            w_mult: 1,
            static_text: None,
            is_static_text: false,
            dax_value: None,
            source_line: None,
            line_length: 0,
            line_width: 0,
            line_vertical: false,
            barcode_height: 0,
            barcode_module: 0,
            barcode_type: None,
            rotation: 0,
            weight_code: 0,
            point_size: None,
            graphic_id: None,
        }
    }

    fn raw_id_prefix(kind: &ElementKind) -> &'static str {
        match kind {
            ElementKind::Text => "H",
            ElementKind::Line => "L",
            ElementKind::Box => "W",
            ElementKind::Barcode => "B",
            ElementKind::Graphic => "U",
        }
    }

    fn add_element_with<F>(&mut self, ctx: &egui::Context, build: F)
    where
        F: FnOnce(u32) -> TaxElement,
    {
        self.ensure_doc();
        if let Some(snapshot) = self.doc.clone() {
            self.push_undo_snapshot(snapshot);
        }
        if let Some(doc) = self.doc.as_mut() {
            let id = doc.next_available_field_id();
            let el = build(id);
            let label = format!("{} ({})", el.raw_id, el.name);
            doc.elements.push(el);
            self.selected_index = Some(doc.elements.len() - 1);
            self.canvas_drag = None;
            self.inspector_undo_snapshot = None;
            self.status_message = format!("Elemen ditambahkan: {label}");
        }
        self.rebuild_preview(ctx);
    }

    fn add_text_element(&mut self, ctx: &egui::Context) {
        self.add_element_with(ctx, |id| {
            let mut el =
                Self::make_base_element(format!("H{id}"), format!("TXT{id:03}"), ElementKind::Text);
            el.h_mult = 1;
            el.w_mult = 1;
            el.weight_code = 20;
            el.is_static_text = true;
            el.static_text = Some("TEKS BARU".into());
            el
        });
    }

    fn add_line_element(&mut self, ctx: &egui::Context) {
        self.add_element_with(ctx, |id| {
            let mut el =
                Self::make_base_element(format!("L{id}"), format!("L{id}"), ElementKind::Line);
            el.line_length = 200;
            el.line_width = 4;
            el
        });
    }

    fn add_box_element(&mut self, ctx: &egui::Context) {
        self.add_element_with(ctx, |id| {
            let mut el =
                Self::make_base_element(format!("W{id}"), format!("W{id}"), ElementKind::Box);
            el.line_length = 220;
            el.h_mult = 120;
            el.line_width = 4;
            el
        });
    }

    fn add_barcode_element(&mut self, ctx: &egui::Context) {
        self.add_element_with(ctx, |id| {
            let mut el = Self::make_base_element(
                format!("B{id}"),
                format!("BAR{id}"),
                ElementKind::Barcode,
            );
            el.barcode_height = 60;
            el.barcode_module = 3;
            el.barcode_type = Some("CODE39".into());
            el.is_static_text = true;
            el.static_text = Some("12345678".into());
            el
        });
    }

    fn add_qr_element(&mut self, ctx: &egui::Context) {
        self.add_element_with(ctx, |id| {
            let mut el = Self::make_base_element(
                format!("B{id}"),
                format!("QR{id}"),
                ElementKind::Barcode,
            );
            el.barcode_height = 80;
            el.barcode_module = 4;
            el.barcode_type = Some("QR".into());
            el.is_static_text = true;
            el.static_text = Some("https://example.com".into());
            el
        });
    }

    fn add_graphic_element(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg", "bmp"])
            .pick_file()
        else {
            return;
        };

        let img = match image::open(&path) {
            Ok(img) => img,
            Err(err) => {
                self.status_message = format!("Gagal membuka gambar: {err}");
                return;
            }
        };

        let img = if img.width() > 320 || img.height() > 320 {
            img.resize(320, 320, image::imageops::FilterType::Lanczos3)
        } else {
            img
        };

        let gray = img.to_luma8();
        let width = gray.width() as usize;
        let height = gray.height() as usize;
        let pixels: Vec<bool> = gray.pixels().map(|p| p.0[0] < 140).collect();

        self.ensure_doc();
        if let Some(snapshot) = self.doc.clone() {
            self.push_undo_snapshot(snapshot);
        }

        if let Some(doc) = self.doc.as_mut() {
            let gid = doc.next_available_graphic_id();
            doc.graphics.insert(
                gid,
                GraphicBitmap {
                    id: gid,
                    width,
                    height,
                    pixels,
                },
            );
            doc.new_graphic_ids.push(gid);

            let id = doc.next_available_field_id();
            let mut el = Self::make_base_element(
                format!("U{id}"),
                format!("{gid:04}"),
                ElementKind::Graphic,
            );
            el.graphic_id = Some(gid);
            el.x = 40;
            el.y = 40;
            el.h_mult = 1;
            el.w_mult = 1;
            doc.elements.push(el);
            self.selected_index = Some(doc.elements.len() - 1);
            self.canvas_drag = None;
            self.inspector_undo_snapshot = None;
            self.status_message = format!("Grafis ditambahkan: U{id} (G{gid:04}, {width}x{height} px)");
        }
        self.rebuild_preview(ctx);
    }

    fn duplicate_selected_element(&mut self, ctx: &egui::Context) {
        let Some(sel_idx) = self.selected_index else {
            return;
        };
        let Some(snapshot) = self.doc.clone() else {
            return;
        };
        if sel_idx >= snapshot.elements.len() {
            self.selected_index = None;
            return;
        }
        self.push_undo_snapshot(snapshot);

        if let Some(doc) = self.doc.as_mut() {
            let new_id = doc.next_available_field_id();
            let mut cloned = doc.elements[sel_idx].clone();
            let prefix = Self::raw_id_prefix(&cloned.kind);
            cloned.source_line = None;
            cloned.raw_id = format!("{prefix}{new_id}");
            cloned.x += 20;
            cloned.y += 20;
            let label = cloned.raw_id.clone();
            doc.elements.push(cloned);
            self.selected_index = Some(doc.elements.len() - 1);
            self.canvas_drag = None;
            self.inspector_undo_snapshot = None;
            self.status_message = format!("Elemen diduplikasi: {label}");
        }
        self.rebuild_preview(ctx);
    }

    fn delete_selected_element(&mut self, ctx: &egui::Context) {
        let Some(sel_idx) = self.selected_index else {
            return;
        };
        let Some(snapshot) = self.doc.clone() else {
            return;
        };
        if sel_idx >= snapshot.elements.len() {
            self.selected_index = None;
            return;
        }
        self.push_undo_snapshot(snapshot);

        if let Some(doc) = self.doc.as_mut() {
            let removed = doc.elements.remove(sel_idx);
            if let Some(line) = removed.source_line {
                doc.deleted_source_lines.push(line);
            }
            self.selected_index = None;
            self.canvas_drag = None;
            self.inspector_undo_snapshot = None;
            self.status_message = format!("Elemen dihapus: {}", removed.raw_id);
        }
        self.rebuild_preview(ctx);
    }

    fn load_file(&mut self, ctx: &egui::Context, path: PathBuf) {
        match std::fs::read(&path) {
            Ok(bytes) => {
                let raw = String::from_utf8_lossy(&bytes).into_owned();
                let doc = parser::parse_label_content(&raw);

                if self.auto_size && doc.detected_width > 0 && doc.detected_height > 0 {
                    self.canvas_width = (doc.detected_width + 40).max(200);
                    self.canvas_height = (doc.detected_height + 40).max(200);
                } else {
                    self.canvas_width = DEFAULT_PAPER_WIDTH;
                    self.canvas_height = DEFAULT_PAPER_HEIGHT;
                }

                let elem_count = doc.elements.len();
                let gfx_count = doc.graphics.len();
                let file_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");
                self.status_message = format!(
                    "Loaded: {file_name} | {elem_count} elemen, {gfx_count} grafis bitmap | Kanvas: {}x{} dots",
                    self.canvas_width, self.canvas_height
                );

                self.current_path = Some(path);
                self.doc = Some(doc);
                self.selected_index = None;
                self.canvas_drag = None;
                self.inspector_undo_snapshot = None;
                self.undo_stack.clear();
                self.redo_stack.clear();
                self.rebuild_preview(ctx);
                self.fit_requested = true;
            }
            Err(err) => {
                self.status_message = format!("Gagal membaca file: {err}");
            }
        }
    }

    fn rebuild_preview(&mut self, ctx: &egui::Context) {
        let Some(doc) = &self.doc else {
            return;
        };

        let (w, h) = if self.auto_size && doc.detected_width > 0 && doc.detected_height > 0 {
            (
                (doc.detected_width + 40).max(200),
                (doc.detected_height + 40).max(200),
            )
        } else {
            (self.canvas_width.max(100), self.canvas_height.max(100))
        };
        self.canvas_width = w;
        self.canvas_height = h;

        self.svg_cache = svg_renderer::render_to_svg(doc, w, h, self.show_var_names);

        if !self.fontdb_loaded {
            self.usvg_options.fontdb_mut().load_system_fonts();
            self.fontdb_loaded = true;
        }

        match resvg::usvg::Tree::from_str(&self.svg_cache, &self.usvg_options) {
            Ok(tree) => {
                let size = tree.size().to_int_size();
                if let Some(mut pixmap) = resvg::tiny_skia::Pixmap::new(size.width(), size.height())
                {
                    resvg::render(
                        &tree,
                        resvg::tiny_skia::Transform::default(),
                        &mut pixmap.as_mut(),
                    );
                    let image = egui::ColorImage::from_rgba_premultiplied(
                        [size.width() as usize, size.height() as usize],
                        pixmap.data(),
                    );
                    self.texture = Some(ctx.load_texture(
                        "label_preview",
                        Arc::new(image),
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
            Err(err) => {
                self.status_message = format!("Error render SVG: {err}");
            }
        }
    }

    fn default_export_name(&self, ext: &str) -> String {
        self.current_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .map(|s| format!("{s}.{ext}"))
            .unwrap_or_else(|| format!("label.{ext}"))
    }

    fn save_svg(&mut self) {
        if self.svg_cache.is_empty() {
            return;
        }
        let default_name = self.default_export_name("svg");

        if let Some(out_path) = rfd::FileDialog::new()
            .add_filter("SVG Image", &["svg"])
            .set_file_name(&default_name)
            .save_file()
        {
            match std::fs::write(&out_path, &self.svg_cache) {
                Ok(_) => {
                    self.status_message = format!("SVG disimpan ke: {}", out_path.display());
                }
                Err(err) => {
                    self.status_message = format!("Gagal menyimpan SVG: {err}");
                }
            }
        }
    }

    fn save_tax(&mut self) {
        let Some(doc) = &self.doc else {
            return;
        };
        let default_name = self.default_export_name("TAX");
        let content = writer::serialize_tax(doc);

        if let Some(out_path) = rfd::FileDialog::new()
            .add_filter("TAX Layout", &["tax", "TAX"])
            .set_file_name(&default_name)
            .save_file()
        {
            match std::fs::write(&out_path, content.as_bytes()) {
                Ok(_) => {
                    self.status_message = format!(".TAX disimpan ke: {}", out_path.display());
                }
                Err(err) => {
                    self.status_message = format!("Gagal menyimpan .TAX: {err}");
                }
            }
        }
    }

    fn save_dax(&mut self) {
        let Some(doc) = &self.doc else {
            return;
        };
        let default_name = self.default_export_name("DAX");
        let content = writer::serialize_dax(doc);

        if let Some(out_path) = rfd::FileDialog::new()
            .add_filter("DAX Label + Data", &["dax", "DAX"])
            .set_file_name(&default_name)
            .save_file()
        {
            match std::fs::write(&out_path, content.as_bytes()) {
                Ok(_) => {
                    self.status_message = format!(".DAX disimpan ke: {}", out_path.display());
                }
                Err(err) => {
                    self.status_message = format!("Gagal menyimpan .DAX: {err}");
                }
            }
        }
    }

    fn hit_test_element(doc: &LabelDocument, dot_pos: egui::Pos2) -> Option<usize> {
        for (idx, el) in doc.elements.iter().enumerate().rev() {
            if el.raw_id == "L99" {
                continue;
            }
            let (bx, by, bw, bh) = doc.element_bounds(el);
            if dot_pos.x >= bx - HIT_TEST_PADDING_DOTS
                && dot_pos.x <= bx + bw + HIT_TEST_PADDING_DOTS
                && dot_pos.y >= by - HIT_TEST_PADDING_DOTS
                && dot_pos.y <= by + bh + HIT_TEST_PADDING_DOTS
            {
                return Some(idx);
            }
        }
        None
    }

    fn element_kind_tag(kind: &ElementKind) -> &'static str {
        match kind {
            ElementKind::Text => "[T]",
            ElementKind::Barcode => "[B]",
            ElementKind::Line => "[L]",
            ElementKind::Box => "[W]",
            ElementKind::Graphic => "[G]",
        }
    }

    fn element_kind_color(kind: &ElementKind) -> egui::Color32 {
        match kind {
            ElementKind::Text => egui::Color32::from_rgb(56, 189, 248),     // sky blue
            ElementKind::Barcode => egui::Color32::from_rgb(245, 158, 11),  // amber
            ElementKind::Line => egui::Color32::from_rgb(148, 163, 184),    // slate gray
            ElementKind::Box => egui::Color32::from_rgb(168, 85, 247),      // purple
            ElementKind::Graphic => egui::Color32::from_rgb(52, 211, 153),  // emerald green
        }
    }

    fn render_editor_panel(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.add_space(4.0);

        let available_height = ui.available_height();

        // Section 1: Element List Card
        egui::Frame::group(ui.style())
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.heading("Daftar Elemen");
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                    if ui
                        .small_button("+ Teks")
                        .on_hover_text("Tambah elemen teks baru")
                        .clicked()
                    {
                        self.add_text_element(ctx);
                    }
                    if ui
                        .small_button("+ Garis")
                        .on_hover_text("Tambah garis baru")
                        .clicked()
                    {
                        self.add_line_element(ctx);
                    }
                    if ui
                        .small_button("+ Kotak")
                        .on_hover_text("Tambah kotak / bingkai baru")
                        .clicked()
                    {
                        self.add_box_element(ctx);
                    }
                    if ui
                        .small_button("+ Barcode")
                        .on_hover_text("Tambah barcode Code 39 baru")
                        .clicked()
                    {
                        self.add_barcode_element(ctx);
                    }
                    if ui
                        .small_button("+ QR")
                        .on_hover_text("Tambah QR Code baru")
                        .clicked()
                    {
                        self.add_qr_element(ctx);
                    }
                    if ui
                        .small_button("+ Grafis")
                        .on_hover_text("Import gambar (PNG/JPG/BMP) sebagai grafis 1-bit")
                        .clicked()
                    {
                        self.add_graphic_element(ctx);
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let clear_w = if self.element_filter.is_empty() { 0.0 } else { 26.0 };
                    ui.add(
                        egui::TextEdit::singleline(&mut self.element_filter)
                            .hint_text("Filter ID / nama / teks...")
                            .desired_width((ui.available_width() - clear_w).max(80.0)),
                    );
                    if !self.element_filter.is_empty() && ui.button("×").clicked() {
                        self.element_filter.clear();
                    }
                });
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    for (filter, label) in [
                        (ElementFilter::All, "Semua"),
                        (ElementFilter::Text, "Teks"),
                        (ElementFilter::Barcode, "Barcode"),
                        (ElementFilter::Line, "Garis"),
                        (ElementFilter::Box, "Kotak"),
                        (ElementFilter::Graphic, "Grafis"),
                    ] {
                        ui.selectable_value(&mut self.element_kind_filter, filter, label);
                    }
                });
                ui.add_space(4.0);

                let filter_lower = self.element_filter.trim().to_lowercase();
                let list_max_h = (available_height * 0.42).max(120.0);
                egui::ScrollArea::vertical()
                    .id_salt("element_list_scroll")
                    .max_height(list_max_h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if let Some(doc) = &self.doc {
                            let font_id = egui::TextStyle::Body.resolve(ui.style());
                            let text_color = ui.visuals().text_color();
                            for (idx, el) in doc.elements.iter().enumerate() {
                                if el.raw_id == "L99" {
                                    continue;
                                }
                                if !self.element_kind_filter.matches(&el.kind) {
                                    continue;
                                }
                                if !filter_lower.is_empty() {
                                    let text_match = el
                                        .display_text()
                                        .map(|t| t.to_lowercase().contains(&filter_lower))
                                        .unwrap_or(false);
                                    if !el.raw_id.to_lowercase().contains(&filter_lower)
                                        && !el.name.to_lowercase().contains(&filter_lower)
                                        && !text_match
                                    {
                                        continue;
                                    }
                                }
                                let tag = Self::element_kind_tag(&el.kind);
                                let rest = if el.name.is_empty() {
                                    format!(" {}", el.raw_id)
                                } else {
                                    format!(" {} — {}", el.raw_id, el.name)
                                };
                                let mut job = egui::text::LayoutJob::default();
                                job.append(
                                    tag,
                                    0.0,
                                    egui::TextFormat {
                                        font_id: font_id.clone(),
                                        color: Self::element_kind_color(&el.kind),
                                        ..Default::default()
                                    },
                                );
                                job.append(
                                    &rest,
                                    0.0,
                                    egui::TextFormat {
                                        font_id: font_id.clone(),
                                        color: text_color,
                                        ..Default::default()
                                    },
                                );
                                let is_selected = self.selected_index == Some(idx);
                                if ui.selectable_label(is_selected, job).clicked() {
                                    self.selected_index = Some(idx);
                                }
                            }
                        } else {
                            ui.label(
                                "Belum ada dokumen. Buka/drop file .TAX/.DAX atau klik '+' di atas untuk membuat label baru.",
                            );
                        }
                    });
            });

        ui.add_space(6.0);

        // Section 2: Property Inspector Card
        let mut changed = false;
        let mut commit_undo_now = false;
        let mut drag_started = false;
        let mut drag_stopped = false;
        let mut doc_before_opt: Option<LabelDocument> = None;
        let mut pending_duplicate = false;
        let mut pending_delete = false;

        egui::Frame::group(ui.style())
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.heading("Inspector Properti");
                    if self.selected_index.is_some() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button("🗑 Hapus")
                                .on_hover_text("Hapus elemen terpilih (Delete)")
                                .clicked()
                            {
                                pending_delete = true;
                            }
                            if ui
                                .small_button("📋 Duplikat")
                                .on_hover_text("Duplikat elemen terpilih (+20, +20)")
                                .clicked()
                            {
                                pending_duplicate = true;
                            }
                        });
                    }
                });
                ui.add_space(2.0);

                let Some(sel_idx) = self.selected_index else {
                    ui.label("Klik elemen pada kanvas atau daftar di atas untuk mengedit properti.");
                    return;
                };

                let Some(doc_before) = self.doc.clone() else {
                    return;
                };
                if sel_idx >= doc_before.elements.len() {
                    self.selected_index = None;
                    return;
                }
                doc_before_opt = Some(doc_before.clone());

                egui::ScrollArea::vertical()
                    .id_salt("inspector_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let Some(doc) = self.doc.as_mut() else {
                            return;
                        };
                        let el = &mut doc.elements[sel_idx];

                        ui.label(
                            egui::RichText::new(format!(
                                "{} {} ({:?})",
                                Self::element_kind_tag(&el.kind),
                                el.raw_id,
                                el.kind
                            ))
                            .strong(),
                        );
                        if !el.name.is_empty() {
                            ui.label(format!("Nama Field: {}", el.name));
                        }
                        ui.add_space(4.0);

                        egui::Grid::new("inspector_grid")
                            .num_columns(2)
                            .min_col_width(115.0)
                            .spacing([8.0, 6.0])
                            .show(ui, |ui| {
                                // X & Y coordinates
                                ui.label("Posisi X (dot):");
                                let rx = ui.add(egui::DragValue::new(&mut el.x).speed(1.0));
                                drag_started |= rx.drag_started();
                                drag_stopped |= rx.drag_stopped();
                                if rx.changed() {
                                    changed = true;
                                    if !rx.dragged() && self.inspector_undo_snapshot.is_none() {
                                        commit_undo_now = true;
                                    }
                                }
                                ui.end_row();

                                ui.label("Posisi Y (dot):");
                                let ry = ui.add(egui::DragValue::new(&mut el.y).speed(1.0));
                                drag_started |= ry.drag_started();
                                drag_stopped |= ry.drag_stopped();
                                if ry.changed() {
                                    changed = true;
                                    if !ry.dragged() && self.inspector_undo_snapshot.is_none() {
                                        commit_undo_now = true;
                                    }
                                }
                                ui.end_row();

                                // Kind-specific geometry
                                match el.kind {
                                    ElementKind::Line => {
                                        ui.label("Panjang Garis (l):");
                                        let rl = ui.add(
                                            egui::DragValue::new(&mut el.line_length)
                                                .speed(1.0)
                                                .range(1..=10000),
                                        );
                                        drag_started |= rl.drag_started();
                                        drag_stopped |= rl.drag_stopped();
                                        if rl.changed() {
                                            changed = true;
                                            if !rl.dragged() && self.inspector_undo_snapshot.is_none() {
                                                commit_undo_now = true;
                                            }
                                        }
                                        ui.end_row();

                                        ui.label("Tebal Garis (w):");
                                        let rw = ui.add(
                                            egui::DragValue::new(&mut el.line_width)
                                                .speed(0.5)
                                                .range(1..=500),
                                        );
                                        drag_started |= rw.drag_started();
                                        drag_stopped |= rw.drag_stopped();
                                        if rw.changed() {
                                            changed = true;
                                            if !rw.dragged() && self.inspector_undo_snapshot.is_none() {
                                                commit_undo_now = true;
                                            }
                                        }
                                        ui.end_row();
                                    }
                                    ElementKind::Box => {
                                        ui.label("Lebar Kotak (l):");
                                        let rw = ui.add(
                                            egui::DragValue::new(&mut el.line_length)
                                                .speed(1.0)
                                                .range(1..=10000),
                                        );
                                        drag_started |= rw.drag_started();
                                        drag_stopped |= rw.drag_stopped();
                                        if rw.changed() {
                                            changed = true;
                                            if !rw.dragged() && self.inspector_undo_snapshot.is_none() {
                                                commit_undo_now = true;
                                            }
                                        }
                                        ui.end_row();

                                        ui.label("Tinggi Kotak (h):");
                                        let rh = ui.add(
                                            egui::DragValue::new(&mut el.h_mult)
                                                .speed(1.0)
                                                .range(1..=10000),
                                        );
                                        drag_started |= rh.drag_started();
                                        drag_stopped |= rh.drag_stopped();
                                        if rh.changed() {
                                            changed = true;
                                            if !rh.dragged() && self.inspector_undo_snapshot.is_none() {
                                                commit_undo_now = true;
                                            }
                                        }
                                        ui.end_row();

                                        ui.label("Tebal Garis (w):");
                                        let rt = ui.add(
                                            egui::DragValue::new(&mut el.line_width)
                                                .speed(0.5)
                                                .range(1..=500),
                                        );
                                        drag_started |= rt.drag_started();
                                        drag_stopped |= rt.drag_stopped();
                                        if rt.changed() {
                                            changed = true;
                                            if !rt.dragged() && self.inspector_undo_snapshot.is_none() {
                                                commit_undo_now = true;
                                            }
                                        }
                                        ui.end_row();
                                    }
                                    ElementKind::Barcode => {
                                        ui.label("Tipe Barcode:");
                                        ui.horizontal(|ui| {
                                            let is_qr = el.barcode_type.as_deref() == Some("QR");
                                            if ui.selectable_label(!is_qr, "Code 39").clicked()
                                                && is_qr
                                            {
                                                el.barcode_type = Some("CODE39".to_string());
                                                changed = true;
                                                commit_undo_now = true;
                                            }
                                            if ui.selectable_label(is_qr, "QR Code").clicked()
                                                && !is_qr
                                            {
                                                el.barcode_type = Some("QR".to_string());
                                                changed = true;
                                                commit_undo_now = true;
                                            }
                                        });
                                        ui.end_row();

                                        ui.label("Tinggi Barcode (h):");
                                        let rbh = ui.add(
                                            egui::DragValue::new(&mut el.barcode_height)
                                                .speed(1.0)
                                                .range(10..=2000),
                                        );
                                        drag_started |= rbh.drag_started();
                                        drag_stopped |= rbh.drag_stopped();
                                        if rbh.changed() {
                                            changed = true;
                                            if !rbh.dragged() && self.inspector_undo_snapshot.is_none() {
                                                commit_undo_now = true;
                                            }
                                        }
                                        ui.end_row();

                                        ui.label("Lebar Modul (w):");
                                        let rbm = ui.add(
                                            egui::DragValue::new(&mut el.barcode_module)
                                                .speed(0.2)
                                                .range(1..=20),
                                        );
                                        drag_started |= rbm.drag_started();
                                        drag_stopped |= rbm.drag_stopped();
                                        if rbm.changed() {
                                            changed = true;
                                            if !rbm.dragged() && self.inspector_undo_snapshot.is_none() {
                                                commit_undo_now = true;
                                            }
                                        }
                                        ui.end_row();
                                    }
                                    ElementKind::Text | ElementKind::Graphic => {
                                        ui.label("Skala Tinggi (h):");
                                        let rh = ui.add(
                                            egui::DragValue::new(&mut el.h_mult)
                                                .speed(0.2)
                                                .range(1..=20),
                                        );
                                        drag_started |= rh.drag_started();
                                        drag_stopped |= rh.drag_stopped();
                                        if rh.changed() {
                                            changed = true;
                                            if !rh.dragged() && self.inspector_undo_snapshot.is_none() {
                                                commit_undo_now = true;
                                            }
                                        }
                                        ui.end_row();

                                        ui.label("Skala Lebar (w):");
                                        let rw = ui.add(
                                            egui::DragValue::new(&mut el.w_mult)
                                                .speed(0.2)
                                                .range(1..=20),
                                        );
                                        drag_started |= rw.drag_started();
                                        drag_stopped |= rw.drag_stopped();
                                        if rw.changed() {
                                            changed = true;
                                            if !rw.dragged() && self.inspector_undo_snapshot.is_none() {
                                                commit_undo_now = true;
                                            }
                                        }
                                        ui.end_row();
                                    }
                                }

                                // Font / Weight (c) for Text
                                if el.kind == ElementKind::Text {
                                    ui.label("Font / Weight (c):");
                                    ui.horizontal(|ui| {
                                        let presets = [
                                            (2, "c2"),
                                            (3, "c3"),
                                            (20, "c20"),
                                            (21, "c21"),
                                            (22, "c22"),
                                        ];
                                        let current_label = presets
                                            .iter()
                                            .find(|(code, _)| *code == el.weight_code)
                                            .map(|(_, lbl)| (*lbl).to_string())
                                            .unwrap_or_else(|| format!("c{}", el.weight_code));

                                        egui::ComboBox::from_id_salt("font_weight_combo")
                                            .selected_text(current_label)
                                            .width(72.0)
                                            .show_ui(ui, |ui| {
                                                for (code, lbl) in presets {
                                                    if ui
                                                        .selectable_value(&mut el.weight_code, code, lbl)
                                                        .changed()
                                                    {
                                                        changed = true;
                                                        commit_undo_now = true;
                                                    }
                                                }
                                            });

                                        let rc = ui.add(
                                            egui::DragValue::new(&mut el.weight_code)
                                                .speed(0.2)
                                                .range(0..=999),
                                        );
                                        drag_started |= rc.drag_started();
                                        drag_stopped |= rc.drag_stopped();
                                        if rc.changed() {
                                            changed = true;
                                            if !rc.dragged() && self.inspector_undo_snapshot.is_none() {
                                                commit_undo_now = true;
                                            }
                                        }
                                    });
                                    ui.end_row();
                                }

                                // Rotation (1-row 4-column selector)
                                ui.label("Rotasi:");
                                ui.columns(4, |cols| {
                                    for (i, (rot, lbl, tip)) in [
                                        (0, "0°", "f0 (0°)"),
                                        (-90, "-90°", "f1 (-90°)"),
                                        (180, "180°", "f2 (180°)"),
                                        (90, "90°", "f3 (90°)"),
                                    ]
                                    .into_iter()
                                    .enumerate()
                                    {
                                        if cols[i]
                                            .selectable_value(&mut el.rotation, rot, lbl)
                                            .on_hover_text(tip)
                                            .changed()
                                        {
                                            changed = true;
                                            commit_undo_now = true;
                                        }
                                    }
                                });
                                ui.end_row();

                                // Text / Data & Field Name for Text & Barcode
                                if el.kind == ElementKind::Text
                                    || el.kind == ElementKind::Barcode
                                {
                                    ui.label("Nama Field:");
                                    let rn = ui.add(
                                        egui::TextEdit::singleline(&mut el.name)
                                            .desired_width(f32::INFINITY),
                                    );
                                    if rn.gained_focus() {
                                        self.inspector_undo_snapshot = Some(doc_before.clone());
                                    }
                                    if rn.changed() {
                                        changed = true;
                                        commit_undo_now = true;
                                    }
                                    if rn.lost_focus() {
                                        self.inspector_undo_snapshot = None;
                                    }
                                    ui.end_row();

                                    ui.label("Sumber Data:");
                                    ui.horizontal(|ui| {
                                        if ui
                                            .selectable_label(el.is_static_text, "Statis (d3)")
                                            .clicked()
                                            && !el.is_static_text
                                        {
                                            el.is_static_text = true;
                                            if el.static_text.is_none() {
                                                el.static_text = el.dax_value.clone();
                                            }
                                            changed = true;
                                            commit_undo_now = true;
                                        }
                                        if ui
                                            .selectable_label(!el.is_static_text, "Variabel (d0)")
                                            .clicked()
                                            && el.is_static_text
                                        {
                                            el.is_static_text = false;
                                            if el.dax_value.is_none() {
                                                el.dax_value = el.static_text.clone();
                                            }
                                            changed = true;
                                            commit_undo_now = true;
                                        }
                                    });
                                    ui.end_row();

                                    if el.is_static_text {
                                        ui.label("Teks Statis (d3):");
                                        let mut buf = el.static_text.clone().unwrap_or_default();
                                        let rt = ui.add(
                                            egui::TextEdit::singleline(&mut buf)
                                                .desired_width(f32::INFINITY),
                                        );
                                        if rt.gained_focus() {
                                            self.inspector_undo_snapshot =
                                                Some(doc_before.clone());
                                        }
                                        if rt.changed() {
                                            el.static_text = Some(buf);
                                            changed = true;
                                            commit_undo_now = true;
                                        }
                                        if rt.lost_focus() {
                                            self.inspector_undo_snapshot = None;
                                        }
                                        ui.end_row();
                                    } else {
                                        ui.label("Data DAX:");
                                        let mut buf = el.dax_value.clone().unwrap_or_default();
                                        let rt = ui.add(
                                            egui::TextEdit::singleline(&mut buf)
                                                .hint_text("Nilai preview DAX...")
                                                .desired_width(f32::INFINITY),
                                        );
                                        if rt.gained_focus() {
                                            self.inspector_undo_snapshot =
                                                Some(doc_before.clone());
                                        }
                                        if rt.changed() {
                                            el.dax_value = Some(buf);
                                            changed = true;
                                            commit_undo_now = true;
                                        }
                                        if rt.lost_focus() {
                                            self.inspector_undo_snapshot = None;
                                        }
                                        ui.end_row();
                                    }
                                }
                            });
                    });
            });

        if let Some(doc_before) = doc_before_opt {
            if drag_started && self.inspector_undo_snapshot.is_none() {
                self.inspector_undo_snapshot = Some(doc_before.clone());
            }
            if changed {
                if let Some(snap) = self.inspector_undo_snapshot.take() {
                    self.push_undo_snapshot(snap);
                } else if commit_undo_now {
                    self.push_undo_snapshot(doc_before);
                }
            }
            if drag_stopped {
                self.inspector_undo_snapshot = None;
            }
        }

        if changed {
            self.rebuild_preview(ctx);
        }

        if pending_duplicate {
            self.duplicate_selected_element(ctx);
        } else if pending_delete {
            self.delete_selected_element(ctx);
        }
    }
}

impl eframe::App for LabelPreviewApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle Drag & Drop
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if let Some(file) = dropped.first() {
            if let Some(path) = &file.path {
                self.load_file(ctx, path.clone());
            }
        }

        // Global Undo / Redo / Delete keyboard shortcuts (when not typing in a text box)
        if !ctx.wants_keyboard_input() {
            let (undo_pressed, redo_pressed, delete_pressed) = ctx.input(|i| {
                let undo = i.modifiers.command
                    && !i.modifiers.shift
                    && i.key_pressed(egui::Key::Z);
                let redo = (i.modifiers.command && i.key_pressed(egui::Key::Y))
                    || (i.modifiers.command
                        && i.modifiers.shift
                        && i.key_pressed(egui::Key::Z));
                let del = i.key_pressed(egui::Key::Delete);
                (undo, redo, del)
            });
            if undo_pressed {
                self.undo(ctx);
            } else if redo_pressed {
                self.redo(ctx);
            } else if delete_pressed && self.selected_index.is_some() {
                self.delete_selected_element(ctx);
            }
        }

        // Top Toolbar
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;

                if ui
                    .button("📂 Buka")
                    .on_hover_text("Buka File (.DAX / .TAX)")
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Label Files", &["dax", "DAX", "tax", "TAX"])
                        .pick_file()
                    {
                        self.load_file(ctx, path);
                    }
                }

                if ui
                    .add_enabled(self.doc.is_some(), egui::Button::new("💾 .TAX"))
                    .on_hover_text("Simpan sebagai template .TAX")
                    .clicked()
                {
                    self.save_tax();
                }

                if ui
                    .add_enabled(self.doc.is_some(), egui::Button::new("💾 .DAX"))
                    .on_hover_text("Simpan sebagai .DAX (template + data)")
                    .clicked()
                {
                    self.save_dax();
                }

                if ui
                    .add_enabled(!self.svg_cache.is_empty(), egui::Button::new("🖼 SVG"))
                    .on_hover_text("Simpan preview sebagai SVG")
                    .clicked()
                {
                    self.save_svg();
                }

                ui.separator();

                if ui
                    .add_enabled(!self.undo_stack.is_empty(), egui::Button::new("↩ Undo"))
                    .on_hover_text("Undo (Ctrl+Z)")
                    .clicked()
                {
                    self.undo(ctx);
                }

                if ui
                    .add_enabled(!self.redo_stack.is_empty(), egui::Button::new("↪ Redo"))
                    .on_hover_text("Redo (Ctrl+Y)")
                    .clicked()
                {
                    self.redo(ctx);
                }

                ui.separator();

                if ui
                    .checkbox(&mut self.show_var_names, "Nama Var")
                    .on_hover_text("Tampilkan Nama Variabel")
                    .changed()
                {
                    self.rebuild_preview(ctx);
                }

                if ui
                    .checkbox(&mut self.auto_size, "Auto Kanvas")
                    .on_hover_text("Auto-Size Kanvas")
                    .changed()
                {
                    self.rebuild_preview(ctx);
                    self.fit_requested = true;
                }

                ui.separator();
                if ui.button("−").on_hover_text("Perkecil Zoom").clicked() {
                    self.zoom = (self.zoom * 0.8).max(0.05);
                }
                ui.label(format!("{:.0}%", self.zoom * 100.0));
                if ui.button("+").on_hover_text("Perbesar Zoom").clicked() {
                    self.zoom = (self.zoom * 1.25).min(5.0);
                }
                if ui.button("Fit").on_hover_text("Fit Window").clicked() {
                    self.fit_requested = true;
                }
            });
            ui.add_space(2.0);
        });

        // Bottom Status Bar
        egui::TopBottomPanel::bottom("statusbar").show(ctx, |ui| {
            ui.label(&self.status_message);
        });

        // Right Side Panel (Element List + Property Inspector)
        egui::SidePanel::right("editor_panel")
            .resizable(true)
            .default_width(310.0)
            .show(ctx, |ui| {
                self.render_editor_panel(ctx, ui);
            });

        // Main Preview & Interactive Canvas Area
        egui::CentralPanel::default().show(ctx, |ui| {
            let available = ui.available_rect_before_wrap();
            let response = ui.allocate_rect(available, egui::Sense::click_and_drag());

            if response.hovered() {
                let scroll = ctx.input(|i| i.raw_scroll_delta.y);
                if scroll != 0.0 {
                    let factor = if scroll > 0.0 { 1.1 } else { 0.9 };
                    self.zoom = (self.zoom * factor).clamp(0.05, 5.0);
                }
            }

            let painter = ui.painter_at(available);
            painter.rect_filled(available, 0.0, egui::Color32::from_rgb(40, 44, 52));

            if let Some(raw_size) = self.texture.as_ref().map(|t| t.size_vec2()) {
                if self.fit_requested {
                    let pad = 24.0;
                    let avail_w = (available.width() - pad).max(64.0);
                    let avail_h = (available.height() - pad).max(64.0);
                    if raw_size.x > 0.0 && raw_size.y > 0.0 {
                        self.zoom = (avail_w / raw_size.x)
                            .min(avail_h / raw_size.y)
                            .clamp(0.05, 5.0);
                        self.pan = egui::Vec2::ZERO;
                    }
                    self.fit_requested = false;
                }

                let tex_size = raw_size * self.zoom;
                let center = available.center() + self.pan;
                let img_rect = egui::Rect::from_center_size(center, tex_size);

                // Middle or right mouse drag always pans
                if response.dragged_by(egui::PointerButton::Middle)
                    || response.dragged_by(egui::PointerButton::Secondary)
                {
                    self.pan += response.drag_delta();
                }

                // Primary click to select / deselect
                if response.clicked_by(egui::PointerButton::Primary) {
                    if let (Some(pos), Some(doc)) = (response.interact_pointer_pos(), &self.doc) {
                        let dot_pos = egui::pos2(
                            (pos.x - img_rect.min.x) / self.zoom,
                            (pos.y - img_rect.min.y) / self.zoom,
                        );
                        self.selected_index = Self::hit_test_element(doc, dot_pos);
                    }
                }

                // Primary drag start: either start dragging hit element or pan empty canvas
                if response.drag_started_by(egui::PointerButton::Primary) {
                    let press_pos = ctx
                        .input(|i| i.pointer.press_origin())
                        .or_else(|| response.interact_pointer_pos());
                    if let (Some(pos), Some(doc)) = (press_pos, self.doc.clone()) {
                        let dot_pos = egui::pos2(
                            (pos.x - img_rect.min.x) / self.zoom,
                            (pos.y - img_rect.min.y) / self.zoom,
                        );
                        if let Some(hit_idx) = Self::hit_test_element(&doc, dot_pos) {
                            self.selected_index = Some(hit_idx);
                            let el = &doc.elements[hit_idx];
                            self.canvas_drag = Some(CanvasDragState {
                                element_index: hit_idx,
                                start_x: el.x,
                                start_y: el.y,
                                accum_delta: egui::Vec2::ZERO,
                            });
                            self.push_undo_snapshot(doc);
                        } else {
                            self.canvas_drag = None;
                        }
                    }
                }

                // Primary drag update
                if response.dragged_by(egui::PointerButton::Primary) {
                    if let Some(drag) = &mut self.canvas_drag {
                        drag.accum_delta += response.drag_delta() / self.zoom;
                        if let Some(doc) = &mut self.doc {
                            if let Some(el) = doc.elements.get_mut(drag.element_index) {
                                el.x = (drag.start_x as f32 + drag.accum_delta.x).round() as i32;
                                el.y = (drag.start_y as f32 + drag.accum_delta.y).round() as i32;
                            }
                        }
                    } else {
                        self.pan += response.drag_delta();
                    }
                }

                // Primary drag end
                if response.drag_stopped() {
                    if let Some(drag) = self.canvas_drag.take() {
                        let moved = self
                            .doc
                            .as_ref()
                            .and_then(|d| d.elements.get(drag.element_index))
                            .map(|el| {
                                (
                                    el.x != drag.start_x || el.y != drag.start_y,
                                    el.raw_id.clone(),
                                    el.x,
                                    el.y,
                                )
                            });
                        if let Some((true, raw_id, nx, ny)) = moved {
                            self.rebuild_preview(ctx);
                            self.status_message = format!("Dipindahkan {raw_id} ke ({nx}, {ny})");
                        } else {
                            // No actual movement occurred; discard the pre-drag undo snapshot
                            self.undo_stack.pop();
                        }
                    }
                }

                // Hover cursor feedback
                if self.canvas_drag.is_some() {
                    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                } else if response.hovered() {
                    if let (Some(hover_pos), Some(doc)) =
                        (ctx.input(|i| i.pointer.hover_pos()), &self.doc)
                    {
                        let dot_pos = egui::pos2(
                            (hover_pos.x - img_rect.min.x) / self.zoom,
                            (hover_pos.y - img_rect.min.y) / self.zoom,
                        );
                        if Self::hit_test_element(doc, dot_pos).is_some() {
                            ctx.set_cursor_icon(egui::CursorIcon::Grab);
                        }
                    }
                }

                // Draw label preview texture
                if let Some(tex) = &self.texture {
                    painter.image(
                        tex.id(),
                        img_rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }

                // Draw cyan selection rectangle & live (x, y) badge
                if let (Some(sel_idx), Some(doc)) = (self.selected_index, &self.doc) {
                    if let Some(el) = doc.elements.get(sel_idx) {
                        let (bx, by, bw, bh) = doc.element_bounds(el);
                        let sel_rect = egui::Rect::from_min_size(
                            img_rect.min + egui::vec2(bx, by) * self.zoom,
                            egui::vec2(bw.max(4.0), bh.max(4.0)) * self.zoom,
                        );
                        painter.rect_filled(
                            sel_rect,
                            0.0,
                            egui::Color32::from_rgba_unmultiplied(56, 189, 248, 24),
                        );
                        let stroke_w: f32 = if self.canvas_drag.is_some() { 2.0 } else { 1.5 };
                        painter.rect_stroke(
                            sel_rect,
                            0.0,
                            egui::Stroke::new(stroke_w, SELECTION_CYAN),
                        );

                        if self.canvas_drag.is_some() {
                            let badge_text = format!("{} ({}, {})", el.raw_id, el.x, el.y);
                            let badge_pos = sel_rect.min - egui::vec2(0.0, 18.0);
                            let galley = painter.layout_no_wrap(
                                badge_text,
                                egui::FontId::monospace(12.0),
                                egui::Color32::BLACK,
                            );
                            let bg_rect = egui::Rect::from_min_size(
                                badge_pos,
                                galley.size() + egui::vec2(8.0, 4.0),
                            );
                            painter.rect_filled(bg_rect, 3.0, SELECTION_CYAN);
                            painter.galley(
                                bg_rect.min + egui::vec2(4.0, 2.0),
                                galley,
                                egui::Color32::BLACK,
                            );
                        }
                    }
                }
            } else {
                painter.text(
                    available.center(),
                    egui::Align2::CENTER_CENTER,
                    "Drop file .DAX atau .TAX di sini untuk preview",
                    egui::FontId::proportional(18.0),
                    egui::Color32::LIGHT_GRAY,
                );
            }
        });
    }
}
