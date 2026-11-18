#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod model;
mod parser;
mod svg_renderer;

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use model::LabelDocument;
use svg_renderer::{DEFAULT_PAPER_HEIGHT, DEFAULT_PAPER_WIDTH};

fn main() -> eframe::Result<()> {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).ok();
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1180.0, 760.0])
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
            status_message: "Drop file .DAX atau .TAX ke jendela ini, atau klik 'Buka File'.".to_string(),
        }
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
            ((doc.detected_width + 40).max(200), (doc.detected_height + 40).max(200))
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

    fn save_svg(&mut self) {
        if self.svg_cache.is_empty() {
            return;
        }
        let default_name = self
            .current_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .map(|s| format!("{s}.svg"))
            .unwrap_or_else(|| "preview.svg".to_string());

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

        // Top Toolbar
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("📂 Buka File (.DAX / .TAX)").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Label Files", &["dax", "DAX", "tax", "TAX"])
                        .pick_file()
                    {
                        self.load_file(ctx, path);
                    }
                }

                if ui
                    .add_enabled(!self.svg_cache.is_empty(), egui::Button::new("💾 Simpan SVG"))
                    .clicked()
                {
                    self.save_svg();
                }

                ui.separator();

                if ui
                    .checkbox(&mut self.show_var_names, "Tampilkan Nama Variabel")
                    .changed()
                {
                    self.rebuild_preview(ctx);
                }

                if ui
                    .checkbox(&mut self.auto_size, "Auto-Size Kanvas")
                    .changed()
                {
                    self.rebuild_preview(ctx);
                    self.fit_requested = true;
                }

                ui.separator();
                ui.label("Zoom:");
                if ui.button("−").clicked() {
                    self.zoom = (self.zoom * 0.8).max(0.05);
                }
                ui.label(format!("{:.0}%", self.zoom * 100.0));
                if ui.button("+").clicked() {
                    self.zoom = (self.zoom * 1.25).min(5.0);
                }
                if ui.button("Fit Window").clicked() {
                    self.fit_requested = true;
                }
            });
        });

        // Bottom Status Bar
        egui::TopBottomPanel::bottom("statusbar").show(ctx, |ui| {
            ui.label(&self.status_message);
        });

        // Main Preview Area
        egui::CentralPanel::default().show(ctx, |ui| {
            let available = ui.available_rect_before_wrap();
            let response = ui.allocate_rect(available, egui::Sense::drag());

            if response.dragged() {
                self.pan += response.drag_delta();
            }

            if response.hovered() {
                let scroll = ctx.input(|i| i.raw_scroll_delta.y);
                if scroll != 0.0 {
                    let factor = if scroll > 0.0 { 1.1 } else { 0.9 };
                    self.zoom = (self.zoom * factor).clamp(0.05, 5.0);
                }
            }

            let painter = ui.painter_at(available);
            painter.rect_filled(available, 0.0, egui::Color32::from_rgb(40, 44, 52));

            if let Some(tex) = &self.texture {
                if self.fit_requested {
                    let pad = 24.0;
                    let avail_w = (available.width() - pad).max(64.0);
                    let avail_h = (available.height() - pad).max(64.0);
                    let raw_size = tex.size_vec2();
                    if raw_size.x > 0.0 && raw_size.y > 0.0 {
                        self.zoom = (avail_w / raw_size.x)
                            .min(avail_h / raw_size.y)
                            .clamp(0.05, 5.0);
                        self.pan = egui::Vec2::ZERO;
                    }
                    self.fit_requested = false;
                }
                let tex_size = tex.size_vec2() * self.zoom;
                let center = available.center() + self.pan;
                let img_rect = egui::Rect::from_center_size(center, tex_size);
                painter.image(
                    tex.id(),
                    img_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
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
