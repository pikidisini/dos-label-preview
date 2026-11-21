#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod model;
mod parser;
mod svg_renderer;
mod writer;

use std::path::{Path, PathBuf};

use model::{ElementKind, GraphicBitmap, LabelDocument, TaxElement};
use serde::Serialize;
use svg_renderer::{DEFAULT_PAPER_HEIGHT, DEFAULT_PAPER_WIDTH};
use tauri::State;

const DEFAULT_GALLERY_DIR: &str =
    r"C:\Users\fiqih\Documents\0000_TRST\01. ABAP\01. Dokumen\Label\Label PRD\saplabelp";

struct AppState {
    initial_file: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum CliAction {
    OpenGui(Option<PathBuf>),
    ExportPng { input: PathBuf, output: PathBuf },
    Error(String),
}

fn parse_cli_args<I>(args: I) -> CliAction
where
    I: IntoIterator<Item = String>,
{
    let mut input: Option<PathBuf> = None;
    let mut export_png: Option<PathBuf> = None;
    let mut iter = args.into_iter();

    while let Some(arg) = iter.next() {
        if arg == "--export-png" {
            match iter.next() {
                Some(out) if !out.is_empty() => export_png = Some(PathBuf::from(out)),
                _ => {
                    return CliAction::Error(
                        "Flag --export-png membutuhkan path output PNG.".into(),
                    )
                }
            }
        } else if let Some(out) = arg.strip_prefix("--export-png=") {
            if out.is_empty() {
                return CliAction::Error("Flag --export-png= membutuhkan path output PNG.".into());
            }
            export_png = Some(PathBuf::from(out));
        } else if arg.starts_with('-') {
            return CliAction::Error(format!("Argumen tidak dikenal: {arg}"));
        } else if input.is_none() {
            input = Some(PathBuf::from(arg));
        }
    }

    match (export_png, input) {
        (Some(output), Some(input)) => CliAction::ExportPng { input, output },
        (Some(_), None) => CliAction::Error(
            "Flag --export-png membutuhkan path file input (.DAX / .TAX).".into(),
        ),
        (None, maybe_input) => CliAction::OpenGui(maybe_input),
    }
}

fn resolve_auto_canvas(doc: &LabelDocument) -> (i32, i32) {
    if doc.detected_width > 0 && doc.detected_height > 0 {
        (
            (doc.detected_width + 40).max(200),
            (doc.detected_height + 40).max(200),
        )
    } else {
        (DEFAULT_PAPER_WIDTH, DEFAULT_PAPER_HEIGHT)
    }
}

fn render_svg_to_pixmap(svg: &str) -> Result<resvg::tiny_skia::Pixmap, String> {
    let mut usvg_options = resvg::usvg::Options::default();
    usvg_options.fontdb_mut().load_system_fonts();

    let tree = resvg::usvg::Tree::from_str(svg, &usvg_options)
        .map_err(|e| format!("Gagal mem-parsing SVG hasil render: {e}"))?;
    let size = tree.size().to_int_size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height())
        .ok_or_else(|| "Gagal mengalokasikan buffer PNG.".to_string())?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap)
}

fn run_headless_export_png(input: &Path, output: &Path) -> Result<(u32, u32), String> {
    let bytes = std::fs::read(input)
        .map_err(|e| format!("Gagal membaca '{}': {e}", input.display()))?;
    let raw = String::from_utf8_lossy(&bytes).into_owned();
    let doc = parser::parse_label_content(&raw);
    let (w, h) = resolve_auto_canvas(&doc);
    let svg = svg_renderer::render_to_svg(&doc, w, h, false);
    let pixmap = render_svg_to_pixmap(&svg)?;

    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Gagal membuat folder '{}': {e}", parent.display()))?;
        }
    }

    pixmap
        .save_png(output)
        .map_err(|e| format!("Gagal menyimpan PNG '{}': {e}", output.display()))?;

    Ok((pixmap.width(), pixmap.height()))
}

#[derive(Serialize)]
struct LoadedLabel {
    path: String,
    file_name: String,
    doc: LabelDocument,
    auto_width: i32,
    auto_height: i32,
}

#[derive(Serialize)]
struct RenderedPreview {
    svg: String,
    width: i32,
    height: i32,
    bounds: Vec<[f32; 4]>,
    next_field_id: u32,
}

#[derive(Serialize)]
struct GalleryItem {
    file_name: String,
    file_path: String,
    width: i32,
    height: i32,
    element_count: usize,
    graphic_count: usize,
    search_text: String,
}

#[tauri::command]
fn get_initial_file(state: State<AppState>) -> Option<String> {
    state.initial_file.clone()
}

#[tauri::command]
fn get_default_gallery_dir() -> String {
    if Path::new(DEFAULT_GALLERY_DIR).is_dir() {
        DEFAULT_GALLERY_DIR.to_string()
    } else {
        String::new()
    }
}

#[tauri::command]
fn pick_gallery_folder() -> Option<String> {
    rfd::FileDialog::new()
        .pick_folder()
        .map(|p| p.to_string_lossy().into_owned())
}

fn collect_label_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_label_files(&path, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let lower = ext.to_ascii_lowercase();
            if lower == "tax" || lower == "dax" {
                out.push(path);
            }
        }
    }
}

#[tauri::command]
fn scan_gallery_folder(folder: String) -> Result<Vec<GalleryItem>, String> {
    let dir = Path::new(&folder);
    if !dir.is_dir() {
        return Err(format!("Folder tidak ditemukan: {folder}"));
    }

    let mut files = Vec::new();
    collect_label_files(dir, &mut files);
    files.sort();

    let mut items = Vec::with_capacity(files.len());
    for path in files {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let raw = String::from_utf8_lossy(&bytes);
        let doc = parser::parse_label_content(&raw);
        let (w, h) = resolve_auto_canvas(&doc);

        let mut tokens = Vec::new();
        for el in &doc.elements {
            if el.raw_id == "L99" {
                continue;
            }
            if !el.name.is_empty() {
                tokens.push(el.name.clone());
            }
            if let Some(txt) = el.display_text() {
                if !txt.trim().is_empty() {
                    tokens.push(txt.trim().to_string());
                }
            }
        }

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        items.push(GalleryItem {
            file_name,
            file_path: path.to_string_lossy().into_owned(),
            width: w,
            height: h,
            element_count: doc.elements.len(),
            graphic_count: doc.graphics.len(),
            search_text: tokens.join(" "),
        });
    }

    Ok(items)
}

#[tauri::command]
fn render_gallery_thumbnail(path: String, show_var_names: bool) -> Result<RenderedPreview, String> {
    let bytes = std::fs::read(&path).map_err(|e| format!("Gagal membaca '{path}': {e}"))?;
    let raw = String::from_utf8_lossy(&bytes);
    let doc = parser::parse_label_content(&raw);
    let (w, h) = resolve_auto_canvas(&doc);
    let svg = svg_renderer::render_to_svg(&doc, w, h, show_var_names);
    Ok(RenderedPreview {
        svg,
        width: w,
        height: h,
        bounds: Vec::new(),
        next_field_id: doc.next_available_field_id(),
    })
}

#[tauri::command]
fn load_label_file(path: String) -> Result<LoadedLabel, String> {
    let p = PathBuf::from(&path);
    let bytes = std::fs::read(&p).map_err(|e| format!("Gagal membaca '{path}': {e}"))?;
    let raw = String::from_utf8_lossy(&bytes);
    let doc = parser::parse_label_content(&raw);
    let (auto_width, auto_height) = resolve_auto_canvas(&doc);
    let file_name = p
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(LoadedLabel {
        path,
        file_name,
        doc,
        auto_width,
        auto_height,
    })
}

#[tauri::command]
fn pick_and_open_label() -> Result<Option<LoadedLabel>, String> {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("Label Files", &["dax", "DAX", "tax", "TAX"])
        .pick_file()
    else {
        return Ok(None);
    };
    let loaded = load_label_file(path.to_string_lossy().into_owned())?;
    Ok(Some(loaded))
}

#[tauri::command]
fn create_blank_document() -> LabelDocument {
    LabelDocument {
        elements: Vec::new(),
        graphics: Default::default(),
        detected_width: 0,
        detected_height: 0,
        raw_layout_lines: vec![
            "<STX><ESC>P;<ETX>".to_string(),
            "<STX>R<ETX>".to_string(),
        ],
        raw_data_lines: Vec::new(),
        deleted_source_lines: Vec::new(),
        new_graphic_ids: Vec::new(),
        uses_tag_syntax: true,
    }
}

#[tauri::command]
fn render_document_svg(
    doc: LabelDocument,
    canvas_width: i32,
    canvas_height: i32,
    show_var_names: bool,
) -> RenderedPreview {
    let w = canvas_width.max(100);
    let h = canvas_height.max(100);
    let svg = svg_renderer::render_to_svg(&doc, w, h, show_var_names);
    let bounds = doc
        .elements
        .iter()
        .map(|el| {
            let (bx, by, bw, bh) = doc.element_bounds(el);
            [bx, by, bw, bh]
        })
        .collect();
    RenderedPreview {
        svg,
        width: w,
        height: h,
        bounds,
        next_field_id: doc.next_available_field_id(),
    }
}

#[derive(Serialize)]
struct ImportedGraphicResult {
    doc: LabelDocument,
    new_index: usize,
    info: String,
}

#[tauri::command]
fn import_graphic_image(mut doc: LabelDocument) -> Result<Option<ImportedGraphicResult>, String> {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("Images", &["png", "jpg", "jpeg", "bmp"])
        .pick_file()
    else {
        return Ok(None);
    };

    let img = image::open(&path).map_err(|e| format!("Gagal membuka gambar: {e}"))?;
    let img = if img.width() > 320 || img.height() > 320 {
        img.resize(320, 320, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };

    let gray = img.to_luma8();
    let width = gray.width() as usize;
    let height = gray.height() as usize;
    let pixels: Vec<bool> = gray.pixels().map(|p| p.0[0] < 140).collect();

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
    doc.elements.push(TaxElement {
        raw_id: format!("U{id}"),
        name: format!("{gid:04}"),
        kind: ElementKind::Graphic,
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
        line_width: 1,
        line_vertical: false,
        barcode_height: 0,
        barcode_module: 1,
        barcode_type: None,
        rotation: 0,
        weight_code: 20,
        point_size: None,
        graphic_id: Some(gid),
    });

    let new_index = doc.elements.len() - 1;
    Ok(Some(ImportedGraphicResult {
        doc,
        new_index,
        info: format!("Grafis ditambahkan: U{id} (G{gid:04}, {width}x{height} px)"),
    }))
}

#[tauri::command]
fn save_tax_dialog(doc: LabelDocument, default_name: String) -> Result<Option<String>, String> {
    let Some(out_path) = rfd::FileDialog::new()
        .add_filter("TAX Layout", &["tax", "TAX"])
        .set_file_name(&default_name)
        .save_file()
    else {
        return Ok(None);
    };
    let content = writer::serialize_tax(&doc);
    std::fs::write(&out_path, content.as_bytes())
        .map_err(|e| format!("Gagal menyimpan .TAX: {e}"))?;
    Ok(Some(out_path.to_string_lossy().into_owned()))
}

#[tauri::command]
fn save_dax_dialog(doc: LabelDocument, default_name: String) -> Result<Option<String>, String> {
    let Some(out_path) = rfd::FileDialog::new()
        .add_filter("DAX Label", &["dax", "DAX"])
        .set_file_name(&default_name)
        .save_file()
    else {
        return Ok(None);
    };
    let content = writer::serialize_dax(&doc);
    std::fs::write(&out_path, content.as_bytes())
        .map_err(|e| format!("Gagal menyimpan .DAX: {e}"))?;
    Ok(Some(out_path.to_string_lossy().into_owned()))
}

#[tauri::command]
fn save_svg_dialog(svg: String, default_name: String) -> Result<Option<String>, String> {
    let Some(out_path) = rfd::FileDialog::new()
        .add_filter("SVG Image", &["svg"])
        .set_file_name(&default_name)
        .save_file()
    else {
        return Ok(None);
    };
    std::fs::write(&out_path, svg.as_bytes())
        .map_err(|e| format!("Gagal menyimpan SVG: {e}"))?;
    Ok(Some(out_path.to_string_lossy().into_owned()))
}

#[tauri::command]
fn save_png_dialog(svg: String, default_name: String) -> Result<Option<String>, String> {
    let Some(out_path) = rfd::FileDialog::new()
        .add_filter("PNG Image", &["png"])
        .set_file_name(&default_name)
        .save_file()
    else {
        return Ok(None);
    };
    let pixmap = render_svg_to_pixmap(&svg)?;
    pixmap
        .save_png(&out_path)
        .map_err(|e| format!("Gagal menyimpan PNG: {e}"))?;
    Ok(Some(out_path.to_string_lossy().into_owned()))
}

fn main() {
    let initial_file = match parse_cli_args(std::env::args().skip(1)) {
        CliAction::ExportPng { input, output } => match run_headless_export_png(&input, &output) {
            Ok((w, h)) => {
                println!("Exported PNG ({w}x{h}) -> {}", output.display());
                std::process::exit(0);
            }
            Err(err) => {
                eprintln!("Error: {err}");
                std::process::exit(1);
            }
        },
        CliAction::Error(err) => {
            eprintln!("Error: {err}");
            std::process::exit(1);
        }
        CliAction::OpenGui(path) => path.map(|p| p.to_string_lossy().into_owned()),
    };

    tauri::Builder::default()
        .manage(AppState { initial_file })
        .invoke_handler(tauri::generate_handler![
            get_initial_file,
            get_default_gallery_dir,
            pick_gallery_folder,
            scan_gallery_folder,
            render_gallery_thumbnail,
            load_label_file,
            pick_and_open_label,
            create_blank_document,
            render_document_svg,
            import_graphic_image,
            save_tax_dialog,
            save_dax_dialog,
            save_svg_dialog,
            save_png_dialog,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn parses_cli_export_png_both_orders_and_gui_fallback() {
        assert_eq!(
            parse_cli_args(vec![
                "C:\\a\\b.dax".into(),
                "--export-png".into(),
                "C:\\out\\res.png".into()
            ]),
            CliAction::ExportPng {
                input: PathBuf::from("C:\\a\\b.dax"),
                output: PathBuf::from("C:\\out\\res.png"),
            }
        );

        assert_eq!(
            parse_cli_args(vec![
                "--export-png".into(),
                "out.png".into(),
                "in.tax".into()
            ]),
            CliAction::ExportPng {
                input: PathBuf::from("in.tax"),
                output: PathBuf::from("out.png"),
            }
        );

        assert_eq!(
            parse_cli_args(vec!["only_input.dax".into()]),
            CliAction::OpenGui(Some(PathBuf::from("only_input.dax")))
        );

        assert_eq!(parse_cli_args(Vec::<String>::new()), CliAction::OpenGui(None));

        assert!(matches!(
            parse_cli_args(vec!["--export-png".into(), "out.png".into()]),
            CliAction::Error(_)
        ));
    }

    #[test]
    fn headless_export_png_creates_parent_dir_and_valid_png() {
        let base = std::env::temp_dir().join("dos_label_preview_cli_test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let input = base.join("sample.dax");
        let output = base.join("nested").join("dir").join("out.png");
        let dax = "<STX><ESC>P;<ETX>\n<STX>H1,TITLE ;o20,20;f0;h2;w2;c20;d3,TEST LABEL;<ETX>\n<STX>R<ETX>\n";
        std::fs::write(&input, dax).unwrap();

        let (w, h) = run_headless_export_png(&input, &output).expect("headless export should work");
        assert!(w >= 200 && h >= 200);
        let png_bytes = std::fs::read(&output).unwrap();
        assert!(png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));

        let _ = std::fs::remove_dir_all(&base);
    }
}
