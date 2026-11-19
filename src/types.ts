export type ElementKind = "Text" | "Barcode" | "Line" | "Box" | "Graphic";

export interface TaxElement {
  raw_id: string;
  name: string;
  kind: ElementKind;
  x: number;
  y: number;
  font_id: string | null;
  h_mult: number;
  w_mult: number;
  static_text: string | null;
  is_static_text: boolean;
  dax_value: string | null;
  source_line: number | null;
  line_length: number;
  line_width: number;
  line_vertical: boolean;
  barcode_height: number;
  barcode_module: number;
  barcode_type: string | null;
  rotation: number;
  weight_code: number;
  point_size: number | null;
  graphic_id: number | null;
}

export interface GraphicBitmap {
  id: number;
  width: number;
  height: number;
  pixels: boolean[];
}

export interface LabelDocument {
  elements: TaxElement[];
  graphics: Record<string, GraphicBitmap>;
  detected_width: number;
  detected_height: number;
  raw_layout_lines: string[];
  raw_data_lines: string[];
  deleted_source_lines: number[];
  new_graphic_ids: number[];
  uses_tag_syntax: boolean;
}

export interface LoadedLabel {
  path: string;
  file_name: string;
  doc: LabelDocument;
  auto_width: number;
  auto_height: number;
}

export interface RenderedPreview {
  svg: string;
  width: number;
  height: number;
  bounds: [number, number, number, number][];
  next_field_id: number;
}

export interface GalleryItem {
  file_name: string;
  file_path: string;
  width: number;
  height: number;
  element_count: number;
  graphic_count: number;
  search_text: string;
}

export interface ImportedGraphicResult {
  doc: LabelDocument;
  new_index: number;
  info: string;
}
