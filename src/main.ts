import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type {
  ElementKind,
  GalleryItem,
  ImportedGraphicResult,
  LabelDocument,
  LoadedLabel,
  RenderedPreview,
  TaxElement,
} from "./types";

const DOTS_PER_MM = 8.0;
const MAX_UNDO = 100;

interface AppState {
  mode: "gallery" | "editor";
  currentPath: string | null;
  currentFileName: string;
  doc: LabelDocument | null;
  svgCache: string;
  bounds: [number, number, number, number][];
  nextFieldId: number;
  selectedIndex: number | null;
  showVarNames: boolean;
  autoCanvas: boolean;
  showRuler: boolean;
  canvasWidth: number;
  canvasHeight: number;
  zoom: number;
  panX: number;
  panY: number;
  layerSearch: string;
  layerFilter: "All" | ElementKind;
  undoStack: LabelDocument[];
  redoStack: LabelDocument[];
  clipboard: TaxElement | null;
  isDirty: boolean;
  cursorDots: [number, number] | null;

  galleryDir: string;
  galleryItems: GalleryItem[];
  gallerySearch: string;
  galleryShowVars: boolean;
  galleryLoaded: boolean;
}

const state: AppState = {
  mode: "editor",
  currentPath: null,
  currentFileName: "",
  doc: null,
  svgCache: "",
  bounds: [],
  nextFieldId: 1,
  selectedIndex: null,
  showVarNames: false,
  autoCanvas: true,
  showRuler: true,
  canvasWidth: 2177,
  canvasHeight: 813,
  zoom: 0.45,
  panX: 40,
  panY: 40,
  layerSearch: "",
  layerFilter: "All",
  undoStack: [],
  redoStack: [],
  clipboard: null,
  isDirty: false,
  cursorDots: null,

  galleryDir: "",
  galleryItems: [],
  gallerySearch: "",
  galleryShowVars: true,
  galleryLoaded: false,
};

// DOM Elements
const tabGallery = document.getElementById("tab-gallery") as HTMLButtonElement;
const tabEditor = document.getElementById("tab-editor") as HTMLButtonElement;
const galleryView = document.getElementById("gallery-view") as HTMLElement;
const editorView = document.getElementById("editor-view") as HTMLElement;

const btnOpen = document.getElementById("btn-open") as HTMLButtonElement;
const btnSaveTax = document.getElementById("btn-save-tax") as HTMLButtonElement;
const btnSaveDax = document.getElementById("btn-save-dax") as HTMLButtonElement;
const btnSaveSvg = document.getElementById("btn-save-svg") as HTMLButtonElement;
const btnSavePng = document.getElementById("btn-save-png") as HTMLButtonElement;
const btnUndo = document.getElementById("btn-undo") as HTMLButtonElement;
const btnRedo = document.getElementById("btn-redo") as HTMLButtonElement;
const btnCopy = document.getElementById("btn-copy") as HTMLButtonElement;
const btnPaste = document.getElementById("btn-paste") as HTMLButtonElement;

const chkVarNames = document.getElementById("chk-var-names") as HTMLInputElement;
const chkAutoCanvas = document.getElementById("chk-auto-canvas") as HTMLInputElement;
const chkRuler = document.getElementById("chk-ruler") as HTMLInputElement;
const btnZoomOut = document.getElementById("btn-zoom-out") as HTMLButtonElement;
const btnZoomIn = document.getElementById("btn-zoom-in") as HTMLButtonElement;
const btnFit = document.getElementById("btn-fit") as HTMLButtonElement;
const zoomLabel = document.getElementById("zoom-label") as HTMLSpanElement;

const canvasArea = document.querySelector(".canvas-area") as HTMLElement;
const rulerTop = document.getElementById("ruler-top") as HTMLCanvasElement;
const rulerLeft = document.getElementById("ruler-left") as HTMLCanvasElement;
const viewport = document.getElementById("canvas-viewport") as HTMLElement;
const emptyState = document.getElementById("empty-state") as HTMLElement;
const labelStage = document.getElementById("label-stage") as HTMLElement;
const svgHost = document.getElementById("svg-host") as HTMLElement;
const selectionBox = document.getElementById("selection-box") as HTMLElement;
const selectionBadge = document.getElementById("selection-badge") as HTMLElement;
const resizeHandle = document.getElementById("resize-handle") as HTMLElement;

const layerSearch = document.getElementById("layer-search") as HTMLInputElement;
const layerFilters = document.getElementById("layer-filters") as HTMLElement;
const layerList = document.getElementById("layer-list") as HTMLElement;
const layerCount = document.getElementById("layer-count") as HTMLSpanElement;

const propActions = document.getElementById("prop-actions") as HTMLElement;
const btnDuplicate = document.getElementById("btn-duplicate") as HTMLButtonElement;
const btnDelete = document.getElementById("btn-delete") as HTMLButtonElement;
const propsEmpty = document.getElementById("props-empty") as HTMLElement;
const propsForm = document.getElementById("props-form") as HTMLElement;

const cvWDots = document.getElementById("cv-w-dots") as HTMLInputElement;
const cvWMm = document.getElementById("cv-w-mm") as HTMLInputElement;
const cvHDots = document.getElementById("cv-h-dots") as HTMLInputElement;
const cvHMm = document.getElementById("cv-h-mm") as HTMLInputElement;

const statusText = document.getElementById("status-text") as HTMLSpanElement;
const cursorReadout = document.getElementById("cursor-readout") as HTMLSpanElement;

const btnPickFolder = document.getElementById("btn-pick-folder") as HTMLButtonElement;
const btnRefreshGallery = document.getElementById("btn-refresh-gallery") as HTMLButtonElement;
const galleryFolderPath = document.getElementById("gallery-folder-path") as HTMLSpanElement;
const gallerySearch = document.getElementById("gallery-search") as HTMLInputElement;
const chkGalleryVars = document.getElementById("chk-gallery-vars") as HTMLInputElement;
const galleryCount = document.getElementById("gallery-count") as HTMLSpanElement;
const galleryGrid = document.getElementById("gallery-grid") as HTMLElement;

function setStatus(msg: string) {
  statusText.textContent = msg;
}

function updateWindowTitle() {
  const dirty = state.isDirty ? " *" : "";
  const name = state.currentFileName ? ` — ${state.currentFileName}` : "";
  getCurrentWindow()
    .setTitle(`DOS Label Preview & Studio${name}${dirty}`)
    .catch(() => {});
}

function cloneDoc(doc: LabelDocument): LabelDocument {
  return JSON.parse(JSON.stringify(doc));
}

function pushUndo() {
  if (!state.doc) return;
  state.undoStack.push(cloneDoc(state.doc));
  if (state.undoStack.length > MAX_UNDO) {
    state.undoStack.shift();
  }
  state.redoStack = [];
  updateToolbarButtons();
}

function updateToolbarButtons() {
  const hasDoc = state.doc !== null;
  btnSaveTax.disabled = !hasDoc;
  btnSaveDax.disabled = !hasDoc;
  btnSaveSvg.disabled = !state.svgCache;
  btnSavePng.disabled = !state.svgCache;
  btnUndo.disabled = state.undoStack.length === 0;
  btnRedo.disabled = state.redoStack.length === 0;
  btnCopy.disabled = state.selectedIndex === null;
  btnPaste.disabled = state.clipboard === null;
}

function switchMode(mode: "gallery" | "editor") {
  state.mode = mode;
  tabGallery.classList.toggle("active", mode === "gallery");
  tabEditor.classList.toggle("active", mode === "editor");
  galleryView.classList.toggle("hidden", mode !== "gallery");
  editorView.classList.toggle("hidden", mode !== "editor");

  if (mode === "gallery" && !state.galleryLoaded) {
    initGallery();
  } else if (mode === "editor") {
    requestAnimationFrame(() => drawRulers());
  }
}

async function initGallery() {
  if (!state.galleryDir) {
    state.galleryDir = await invoke<string>("get_default_gallery_dir");
  }
  galleryFolderPath.textContent = state.galleryDir || "(Pilih folder template...)";
  if (state.galleryDir) {
    await scanGallery();
  }
}

let galleryObserver: IntersectionObserver | null = null;
const thumbCache = new Map<string, string>();

async function scanGallery() {
  if (!state.galleryDir) return;
  setStatus(`Memindai folder galeri: ${state.galleryDir}...`);
  try {
    state.galleryItems = await invoke<GalleryItem[]>("scan_gallery_folder", {
      folder: state.galleryDir,
    });
    state.galleryLoaded = true;
    thumbCache.clear();
    renderGalleryGrid();
    setStatus(`Galeri siap: ${state.galleryItems.length} file .TAX/.DAX ditemukan.`);
  } catch (err) {
    setStatus(`Gagal memindai galeri: ${err}`);
  }
}

function renderGalleryGrid() {
  if (galleryObserver) {
    galleryObserver.disconnect();
  }
  galleryGrid.innerHTML = "";
  const q = state.gallerySearch.trim().toLowerCase();
  const filtered = state.galleryItems.filter(
    (it) =>
      !q ||
      it.file_name.toLowerCase().includes(q) ||
      it.search_text.toLowerCase().includes(q),
  );
  galleryCount.textContent = `${filtered.length} / ${state.galleryItems.length} label`;

  galleryObserver = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue;
        const card = entry.target as HTMLElement;
        const path = card.dataset.path;
        if (!path) continue;
        galleryObserver?.unobserve(card);
        loadGalleryThumb(card, path);
      }
    },
    { root: galleryGrid, rootMargin: "200px" },
  );

  for (const item of filtered) {
    const card = document.createElement("div");
    card.className = "gallery-card";
    card.dataset.path = item.file_path;

    const thumb = document.createElement("div");
    thumb.className = "gallery-thumb";
    thumb.innerHTML = `<span class="muted">Memuat...</span>`;

    const meta = document.createElement("div");
    meta.className = "gallery-meta";
    const wMm = (item.width / DOTS_PER_MM).toFixed(0);
    const hMm = (item.height / DOTS_PER_MM).toFixed(0);
    meta.innerHTML = `
      <span class="gallery-title" title="${item.file_path}">${item.file_name}</span>
      <span class="gallery-sub">${wMm}×${hMm} mm • ${item.element_count} el</span>
    `;

    card.appendChild(thumb);
    card.appendChild(meta);
    card.addEventListener("click", () => openLabelFile(item.file_path));

    galleryGrid.appendChild(card);
    galleryObserver.observe(card);
  }
}

async function loadGalleryThumb(card: HTMLElement, path: string) {
  const thumb = card.querySelector(".gallery-thumb") as HTMLElement;
  const cacheKey = `${path}::${state.galleryShowVars}`;
  const cached = thumbCache.get(cacheKey);
  if (cached) {
    thumb.innerHTML = cached;
    return;
  }
  try {
    const res = await invoke<RenderedPreview>("render_gallery_thumbnail", {
      path,
      showVarNames: state.galleryShowVars,
    });
    thumbCache.set(cacheKey, res.svg);
    thumb.innerHTML = res.svg;
  } catch {
    thumb.innerHTML = `<span class="muted">Gagal render</span>`;
  }
}

async function openLabelFile(path: string) {
  try {
    const loaded = await invoke<LoadedLabel>("load_label_file", { path });
    applyLoadedLabel(loaded);
    switchMode("editor");
  } catch (err) {
    setStatus(`Gagal membuka file: ${err}`);
  }
}

function applyLoadedLabel(loaded: LoadedLabel) {
  state.currentPath = loaded.path;
  state.currentFileName = loaded.file_name;
  state.doc = loaded.doc;
  state.selectedIndex = null;
  state.undoStack = [];
  state.redoStack = [];
  state.isDirty = false;
  if (state.autoCanvas) {
    state.canvasWidth = loaded.auto_width;
    state.canvasHeight = loaded.auto_height;
  }
  updateWindowTitle();
  syncCanvasInputs();
  rebuildPreview(true);
  setStatus(
    `Loaded: ${loaded.file_name} | ${loaded.doc.elements.length} elemen, ${Object.keys(loaded.doc.graphics).length} grafis`,
  );
}

async function ensureDoc(): Promise<LabelDocument> {
  if (!state.doc) {
    state.doc = await invoke<LabelDocument>("create_blank_document");
    state.currentFileName = "untitled.TAX";
    state.canvasWidth = 1200;
    state.canvasHeight = 600;
    state.autoCanvas = false;
    chkAutoCanvas.checked = false;
    syncCanvasInputs();
  }
  return state.doc;
}

async function rebuildPreview(fitAfter = false) {
  if (!state.doc) {
    emptyState.classList.remove("hidden");
    labelStage.classList.add("hidden");
    updateToolbarButtons();
    renderLayers();
    renderProperties();
    return;
  }

  emptyState.classList.add("hidden");
  labelStage.classList.remove("hidden");

  if (state.autoCanvas && state.doc.detected_width > 0 && state.doc.detected_height > 0) {
    state.canvasWidth = Math.max(200, state.doc.detected_width + 40);
    state.canvasHeight = Math.max(200, state.doc.detected_height + 40);
    syncCanvasInputs();
  }

  const res = await invoke<RenderedPreview>("render_document_svg", {
    doc: state.doc,
    canvasWidth: state.canvasWidth,
    canvasHeight: state.canvasHeight,
    showVarNames: state.showVarNames,
  });

  state.svgCache = res.svg;
  state.bounds = res.bounds;
  state.nextFieldId = res.next_field_id;
  svgHost.innerHTML = res.svg;

  if (fitAfter) {
    fitToWindow();
  } else {
    applyStageTransform();
  }

  updateToolbarButtons();
  renderLayers();
  renderProperties();
  updateSelectionOverlay();
}

function fitToWindow() {
  const rect = viewport.getBoundingClientRect();
  if (rect.width <= 0 || rect.height <= 0) return;
  const pad = 32;
  const zx = (rect.width - pad) / state.canvasWidth;
  const zy = (rect.height - pad) / state.canvasHeight;
  state.zoom = Math.max(0.05, Math.min(5.0, Math.min(zx, zy)));
  state.panX = (rect.width - state.canvasWidth * state.zoom) / 2;
  state.panY = (rect.height - state.canvasHeight * state.zoom) / 2;
  applyStageTransform();
}

function applyStageTransform() {
  labelStage.style.transform = `translate(${state.panX}px, ${state.panY}px) scale(${state.zoom})`;
  zoomLabel.textContent = `${Math.round(state.zoom * 100)}%`;
  updateSelectionOverlay();
  drawRulers();
}

function updateSelectionOverlay() {
  if (
    state.selectedIndex === null ||
    !state.doc ||
    state.selectedIndex >= state.bounds.length
  ) {
    selectionBox.classList.add("hidden");
    return;
  }
  const el = state.doc.elements[state.selectedIndex];
  const [bx, by, bw, bh] = state.bounds[state.selectedIndex];
  const pad = 4;
  selectionBox.classList.remove("hidden");
  selectionBox.style.left = `${bx - pad}px`;
  selectionBox.style.top = `${by - pad}px`;
  selectionBox.style.width = `${Math.max(8, bw + pad * 2)}px`;
  selectionBox.style.height = `${Math.max(8, bh + pad * 2)}px`;

  const invScale = 1 / state.zoom;
  selectionBadge.textContent = `${el.raw_id} (${el.x}, ${el.y})`;
  selectionBadge.style.transform = `scale(${invScale})`;
  selectionBadge.style.transformOrigin = "bottom left";
  resizeHandle.style.transform = `scale(${invScale})`;
}

function drawRulers() {
  if (!state.showRuler || state.mode !== "editor") return;
  const pxPerMm = DOTS_PER_MM * state.zoom;
  const steps = [1, 2, 5, 10, 20, 50, 100];
  const majorMm = steps.find((s) => s * pxPerMm >= 48) ?? 100;
  const minorMm = majorMm / 5;

  // Top Ruler
  const topRect = rulerTop.getBoundingClientRect();
  rulerTop.width = Math.max(1, Math.floor(topRect.width));
  rulerTop.height = 22;
  const ctxT = rulerTop.getContext("2d");
  if (ctxT) {
    ctxT.fillStyle = "#1a1d24";
    ctxT.fillRect(0, 0, rulerTop.width, 22);
    const startMm = Math.floor(-state.panX / pxPerMm / majorMm) * majorMm;
    const endMm = Math.ceil((rulerTop.width - state.panX) / pxPerMm);
    ctxT.strokeStyle = "#4b5563";
    ctxT.fillStyle = "#9ca3af";
    ctxT.font = "10px Consolas, monospace";
    for (let mm = startMm; mm <= endMm; mm += minorMm) {
      const x = Math.round(state.panX + mm * pxPerMm) + 0.5;
      const isMajor = Math.abs(Math.round(mm / majorMm) * majorMm - mm) < 0.001;
      ctxT.beginPath();
      ctxT.moveTo(x, isMajor ? 4 : 14);
      ctxT.lineTo(x, 22);
      ctxT.stroke();
      if (isMajor) {
        ctxT.fillText(`${Math.round(mm)}`, x + 3, 12);
      }
    }
    if (state.cursorDots) {
      const cx = state.panX + state.cursorDots[0] * state.zoom;
      ctxT.strokeStyle = "#38bdf8";
      ctxT.beginPath();
      ctxT.moveTo(cx, 0);
      ctxT.lineTo(cx, 22);
      ctxT.stroke();
    }
  }

  // Left Ruler
  const leftRect = rulerLeft.getBoundingClientRect();
  rulerLeft.width = 26;
  rulerLeft.height = Math.max(1, Math.floor(leftRect.height));
  const ctxL = rulerLeft.getContext("2d");
  if (ctxL) {
    ctxL.fillStyle = "#1a1d24";
    ctxL.fillRect(0, 0, 26, rulerLeft.height);
    const startMm = Math.floor(-state.panY / pxPerMm / majorMm) * majorMm;
    const endMm = Math.ceil((rulerLeft.height - state.panY) / pxPerMm);
    ctxL.strokeStyle = "#4b5563";
    ctxL.fillStyle = "#9ca3af";
    ctxL.font = "10px Consolas, monospace";
    for (let mm = startMm; mm <= endMm; mm += minorMm) {
      const y = Math.round(state.panY + mm * pxPerMm) + 0.5;
      const isMajor = Math.abs(Math.round(mm / majorMm) * majorMm - mm) < 0.001;
      ctxL.beginPath();
      ctxL.moveTo(isMajor ? 4 : 16, y);
      ctxL.lineTo(26, y);
      ctxL.stroke();
      if (isMajor) {
        ctxL.save();
        ctxL.translate(11, y - 2);
        ctxL.rotate(-Math.PI / 2);
        ctxL.fillText(`${Math.round(mm)}`, 0, 0);
        ctxL.restore();
      }
    }
    if (state.cursorDots) {
      const cy = state.panY + state.cursorDots[1] * state.zoom;
      ctxL.strokeStyle = "#38bdf8";
      ctxL.beginPath();
      ctxL.moveTo(0, cy);
      ctxL.lineTo(26, cy);
      ctxL.stroke();
    }
  }
}

function kindShortTag(kind: ElementKind): string {
  switch (kind) {
    case "Text": return "T";
    case "Barcode": return "B";
    case "Line": return "L";
    case "Box": return "W";
    case "Graphic": return "G";
  }
}

function renderLayers() {
  layerList.innerHTML = "";
  if (!state.doc) {
    layerCount.textContent = "0";
    return;
  }
  const q = state.layerSearch.trim().toLowerCase();
  let visible = 0;

  state.doc.elements.forEach((el, idx) => {
    if (el.raw_id === "L99") return;
    if (state.layerFilter !== "All" && el.kind !== state.layerFilter) return;
    const hay = `${el.raw_id} ${el.name} ${el.static_text ?? ""} ${el.dax_value ?? ""}`.toLowerCase();
    if (q && !hay.includes(q)) return;

    visible++;
    const item = document.createElement("div");
    item.className = `layer-item${state.selectedIndex === idx ? " selected" : ""}`;
    const tag = kindShortTag(el.kind);
    const label = el.name ? `${el.raw_id} — ${el.name}` : el.raw_id;
    item.innerHTML = `<span class="tag-${tag}">[${tag}]</span><span>${label}</span>`;
    item.addEventListener("click", () => {
      state.selectedIndex = idx;
      renderLayers();
      renderProperties();
      updateSelectionOverlay();
      updateToolbarButtons();
    });
    layerList.appendChild(item);
  });
  layerCount.textContent = `${visible}`;
}

function addPropRow(label: string, inputEl: HTMLElement) {
  const lbl = document.createElement("label");
  lbl.textContent = label;
  propsForm.appendChild(lbl);
  propsForm.appendChild(inputEl);
}

function makeNumberInput(val: number, min: number, max: number, onChange: (n: number) => void): HTMLInputElement {
  const inp = document.createElement("input");
  inp.type = "number";
  inp.value = `${val}`;
  inp.min = `${min}`;
  inp.max = `${max}`;
  inp.addEventListener("change", () => {
    const parsed = parseInt(inp.value, 10);
    if (!Number.isNaN(parsed)) {
      pushUndo();
      onChange(Math.max(min, Math.min(max, parsed)));
      state.isDirty = true;
      updateWindowTitle();
      rebuildPreview();
    }
  });
  return inp;
}

function renderProperties() {
  propsForm.innerHTML = "";
  if (
    state.selectedIndex === null ||
    !state.doc ||
    state.selectedIndex >= state.doc.elements.length
  ) {
    propActions.classList.add("hidden");
    propsEmpty.classList.remove("hidden");
    propsForm.classList.add("hidden");
    return;
  }

  const el = state.doc.elements[state.selectedIndex];
  propActions.classList.remove("hidden");
  propsEmpty.classList.add("hidden");
  propsForm.classList.remove("hidden");

  addPropRow("ID / Tipe:", (() => {
    const s = document.createElement("span");
    s.textContent = `${el.raw_id} (${el.kind})`;
    s.style.fontWeight = "600";
    return s;
  })());

  addPropRow("Posisi X (dot):", makeNumberInput(el.x, -500, 10000, (n) => { el.x = n; }));
  addPropRow("Posisi Y (dot):", makeNumberInput(el.y, -500, 10000, (n) => { el.y = n; }));

  if (el.kind === "Line") {
    addPropRow("Panjang (l):", makeNumberInput(el.line_length, 1, 10000, (n) => { el.line_length = n; }));
    addPropRow("Tebal (w):", makeNumberInput(el.line_width, 1, 500, (n) => { el.line_width = n; }));
  } else if (el.kind === "Box") {
    addPropRow("Lebar Kotak (l):", makeNumberInput(el.line_length, 1, 10000, (n) => { el.line_length = n; }));
    addPropRow("Tinggi Kotak (h):", makeNumberInput(el.h_mult, 1, 10000, (n) => { el.h_mult = n; }));
    addPropRow("Tebal Garis (w):", makeNumberInput(el.line_width, 1, 500, (n) => { el.line_width = n; }));
  } else if (el.kind === "Barcode") {
    const isQr = el.barcode_type === "QR";
    const row = document.createElement("div");
    row.className = "seg-row";
    for (const [val, label] of [["CODE39", "Code 39"], ["QR", "QR Code"]] as const) {
      const b = document.createElement("button");
      b.className = `seg-btn${(isQr ? "QR" : "CODE39") === val ? " active" : ""}`;
      b.textContent = label;
      b.addEventListener("click", () => {
        pushUndo();
        el.barcode_type = val;
        state.isDirty = true;
        updateWindowTitle();
        rebuildPreview();
      });
      row.appendChild(b);
    }
    addPropRow("Tipe Barcode:", row);

    if (isQr) {
      addPropRow("Modul QR (w):", makeNumberInput(el.barcode_module, 1, 30, (n) => { el.barcode_module = n; }));
    } else {
      addPropRow("Tinggi (h):", makeNumberInput(el.barcode_height, 10, 2000, (n) => { el.barcode_height = n; }));
      addPropRow("Modul (w):", makeNumberInput(el.barcode_module, 1, 20, (n) => { el.barcode_module = n; }));
    }
  } else {
    addPropRow("Skala Tinggi (h):", makeNumberInput(el.h_mult, 1, 20, (n) => { el.h_mult = n; }));
    addPropRow("Skala Lebar (w):", makeNumberInput(el.w_mult, 1, 20, (n) => { el.w_mult = n; }));
  }

  if (el.kind === "Text") {
    const sel = document.createElement("select");
    for (const c of [2, 3, 20, 21, 22]) {
      const opt = document.createElement("option");
      opt.value = `${c}`;
      opt.textContent = `c${c}`;
      if (el.weight_code === c) opt.selected = true;
      sel.appendChild(opt);
    }
    if (![2, 3, 20, 21, 22].includes(el.weight_code)) {
      const opt = document.createElement("option");
      opt.value = `${el.weight_code}`;
      opt.textContent = `c${el.weight_code}`;
      opt.selected = true;
      sel.appendChild(opt);
    }
    sel.addEventListener("change", () => {
      pushUndo();
      el.weight_code = parseInt(sel.value, 10);
      state.isDirty = true;
      updateWindowTitle();
      rebuildPreview();
    });
    addPropRow("Font (c):", sel);
  }

  // Rotation
  const rotRow = document.createElement("div");
  rotRow.className = "seg-row";
  for (const [deg, lbl] of [[0, "0°"], [-90, "-90°"], [180, "180°"], [90, "90°"]] as const) {
    const b = document.createElement("button");
    b.className = `seg-btn${el.rotation === deg ? " active" : ""}`;
    b.textContent = lbl;
    b.addEventListener("click", () => {
      pushUndo();
      el.rotation = deg;
      state.isDirty = true;
      updateWindowTitle();
      rebuildPreview();
    });
    rotRow.appendChild(b);
  }
  addPropRow("Rotasi:", rotRow);

  if (el.kind === "Text" || el.kind === "Barcode") {
    const nameInp = document.createElement("input");
    nameInp.type = "text";
    nameInp.value = el.name;
    nameInp.addEventListener("change", () => {
      pushUndo();
      el.name = nameInp.value;
      state.isDirty = true;
      updateWindowTitle();
      rebuildPreview();
    });
    addPropRow("Nama Field:", nameInp);

    const srcRow = document.createElement("div");
    srcRow.className = "seg-row";
    for (const [isStat, lbl] of [[true, "Statis (d3)"], [false, "Variabel (d0)"]] as const) {
      const b = document.createElement("button");
      b.className = `seg-btn${el.is_static_text === isStat ? " active" : ""}`;
      b.textContent = lbl;
      b.addEventListener("click", () => {
        pushUndo();
        el.is_static_text = isStat;
        state.isDirty = true;
        updateWindowTitle();
        rebuildPreview();
      });
      srcRow.appendChild(b);
    }
    addPropRow("Sumber Data:", srcRow);

    const txtInp = document.createElement("input");
    txtInp.type = "text";
    txtInp.value = el.is_static_text ? el.static_text ?? "" : el.dax_value ?? "";
    txtInp.placeholder = el.is_static_text ? "Teks statis..." : "Nilai preview DAX...";
    txtInp.addEventListener("input", () => {
      if (el.is_static_text) {
        el.static_text = txtInp.value;
      } else {
        el.dax_value = txtInp.value;
      }
      state.isDirty = true;
      updateWindowTitle();
      rebuildPreview();
    });
    addPropRow(el.is_static_text ? "Teks Statis:" : "Data DAX:", txtInp);
  }
}

function syncCanvasInputs() {
  cvWDots.value = `${state.canvasWidth}`;
  cvWMm.value = (state.canvasWidth / DOTS_PER_MM).toFixed(1);
  cvHDots.value = `${state.canvasHeight}`;
  cvHMm.value = (state.canvasHeight / DOTS_PER_MM).toFixed(1);
}

// Element Creation
async function addNewElement(kind: "text" | "line" | "box" | "barcode" | "qr") {
  const doc = await ensureDoc();
  pushUndo();
  const id = state.nextFieldId || 1;
  const base: TaxElement = {
    raw_id: `H${id}`,
    name: `TXT${String(id).padStart(3, "0")}`,
    kind: "Text",
    x: 40,
    y: 40,
    font_id: null,
    h_mult: 1,
    w_mult: 1,
    static_text: null,
    is_static_text: true,
    dax_value: null,
    source_line: null,
    line_length: 0,
    line_width: 1,
    line_vertical: false,
    barcode_height: 0,
    barcode_module: 1,
    barcode_type: null,
    rotation: 0,
    weight_code: 20,
    point_size: null,
    graphic_id: null,
  };

  if (kind === "text") {
    base.static_text = "TEKS BARU";
  } else if (kind === "line") {
    base.kind = "Line";
    base.raw_id = `L${id}`;
    base.name = `L${id}`;
    base.line_length = 200;
    base.line_width = 4;
  } else if (kind === "box") {
    base.kind = "Box";
    base.raw_id = `W${id}`;
    base.name = `W${id}`;
    base.line_length = 220;
    base.h_mult = 120;
    base.line_width = 4;
  } else if (kind === "barcode") {
    base.kind = "Barcode";
    base.raw_id = `B${id}`;
    base.name = `BAR${id}`;
    base.barcode_height = 60;
    base.barcode_module = 3;
    base.barcode_type = "CODE39";
    base.static_text = "12345678";
  } else if (kind === "qr") {
    base.kind = "Barcode";
    base.raw_id = `B${id}`;
    base.name = `QR${id}`;
    base.barcode_height = 80;
    base.barcode_module = 4;
    base.barcode_type = "QR";
    base.static_text = "https://example.com";
  }

  doc.elements.push(base);
  state.selectedIndex = doc.elements.length - 1;
  state.isDirty = true;
  updateWindowTitle();
  await rebuildPreview();
  setStatus(`Ditambahkan: ${base.raw_id}`);
}

async function importGraphic() {
  const doc = await ensureDoc();
  try {
    const res = await invoke<ImportedGraphicResult | null>("import_graphic_image", { doc });
    if (!res) return;
    pushUndo();
    state.doc = res.doc;
    state.selectedIndex = res.new_index;
    state.isDirty = true;
    updateWindowTitle();
    await rebuildPreview();
    setStatus(res.info);
  } catch (err) {
    setStatus(`Gagal import grafis: ${err}`);
  }
}

function duplicateOrPaste(cloned: TaxElement, msg: string) {
  if (!state.doc) return;
  pushUndo();
  const id = state.nextFieldId || 1;
  const prefix = kindShortTag(cloned.kind);
  const copy = JSON.parse(JSON.stringify(cloned)) as TaxElement;
  copy.source_line = null;
  copy.raw_id = `${prefix}${id}`;
  copy.x += 20;
  copy.y += 20;
  state.doc.elements.push(copy);
  state.selectedIndex = state.doc.elements.length - 1;
  state.isDirty = true;
  updateWindowTitle();
  rebuildPreview();
  setStatus(`${msg}: ${copy.raw_id}`);
}

function deleteSelected() {
  if (state.selectedIndex === null || !state.doc) return;
  pushUndo();
  const [removed] = state.doc.elements.splice(state.selectedIndex, 1);
  if (removed.source_line !== null) {
    state.doc.deleted_source_lines.push(removed.source_line);
  }
  state.selectedIndex = null;
  state.isDirty = true;
  updateWindowTitle();
  rebuildPreview();
  setStatus(`Elemen dihapus: ${removed.raw_id}`);
}

function undo() {
  if (state.undoStack.length === 0 || !state.doc) return;
  state.redoStack.push(cloneDoc(state.doc));
  state.doc = state.undoStack.pop()!;
  state.selectedIndex = null;
  state.isDirty = true;
  updateWindowTitle();
  rebuildPreview();
  setStatus("Undo");
}

function redo() {
  if (state.redoStack.length === 0 || !state.doc) return;
  state.undoStack.push(cloneDoc(state.doc));
  state.doc = state.redoStack.pop()!;
  state.selectedIndex = null;
  state.isDirty = true;
  updateWindowTitle();
  rebuildPreview();
  setStatus("Redo");
}

// Mouse / Drag / Resize on Canvas
let dragMode: null | "pan" | "move" | "resize" = null;
let dragStartClient = { x: 0, y: 0 };
let dragOrig = { panX: 0, panY: 0, elX: 0, elY: 0, l: 0, h: 0, w: 0, bh: 0, bm: 0, baseW: 0, baseH: 0 };

function clientToDots(clientX: number, clientY: number): [number, number] {
  const rect = viewport.getBoundingClientRect();
  return [
    (clientX - rect.left - state.panX) / state.zoom,
    (clientY - rect.top - state.panY) / state.zoom,
  ];
}

function hitTest(dx: number, dy: number): number | null {
  if (!state.doc) return null;
  const pad = 6;
  for (let i = state.doc.elements.length - 1; i >= 0; i--) {
    if (state.doc.elements[i].raw_id === "L99") continue;
    const [bx, by, bw, bh] = state.bounds[i];
    if (dx >= bx - pad && dx <= bx + bw + pad && dy >= by - pad && dy <= by + bh + pad) {
      return i;
    }
  }
  return null;
}

resizeHandle.addEventListener("mousedown", (e) => {
  if (e.button !== 0 || state.selectedIndex === null || !state.doc) return;
  e.stopPropagation();
  const el = state.doc.elements[state.selectedIndex];
  const [, , bw, bh] = state.bounds[state.selectedIndex];
  pushUndo();
  dragMode = "resize";
  dragStartClient = { x: e.clientX, y: e.clientY };
  dragOrig = {
    panX: state.panX,
    panY: state.panY,
    elX: el.x,
    elY: el.y,
    l: el.line_length,
    h: el.h_mult,
    w: el.w_mult,
    bh: el.barcode_height,
    bm: el.barcode_module,
    baseW: Math.max(10, bw / Math.max(1, el.w_mult)),
    baseH: Math.max(10, bh / Math.max(1, el.h_mult)),
  };
});

viewport.addEventListener("mousedown", (e) => {
  if (!state.doc) return;
  if (e.button === 1 || e.button === 2) {
    dragMode = "pan";
    dragStartClient = { x: e.clientX, y: e.clientY };
    dragOrig = { ...dragOrig, panX: state.panX, panY: state.panY };
    return;
  }
  if (e.button !== 0) return;

  const [dx, dy] = clientToDots(e.clientX, e.clientY);
  const hit = hitTest(dx, dy);
  if (hit !== null) {
    state.selectedIndex = hit;
    const el = state.doc.elements[hit];
    pushUndo();
    dragMode = "move";
    dragStartClient = { x: e.clientX, y: e.clientY };
    dragOrig = { ...dragOrig, elX: el.x, elY: el.y };
    renderLayers();
    renderProperties();
    updateSelectionOverlay();
    updateToolbarButtons();
  } else {
    state.selectedIndex = null;
    dragMode = "pan";
    dragStartClient = { x: e.clientX, y: e.clientY };
    dragOrig = { ...dragOrig, panX: state.panX, panY: state.panY };
    renderLayers();
    renderProperties();
    updateSelectionOverlay();
    updateToolbarButtons();
  }
});

window.addEventListener("mousemove", (e) => {
  if (state.mode === "editor" && state.doc) {
    const [cx, cy] = clientToDots(e.clientX, e.clientY);
    state.cursorDots = [cx, cy];
    cursorReadout.textContent = `X: ${(cx / DOTS_PER_MM).toFixed(1)} mm (${Math.round(cx)} dot) | Y: ${(cy / DOTS_PER_MM).toFixed(1)} mm (${Math.round(cy)} dot)`;
    drawRulers();
  }

  if (!dragMode) return;
  const dxScreen = e.clientX - dragStartClient.x;
  const dyScreen = e.clientY - dragStartClient.y;

  if (dragMode === "pan") {
    state.panX = dragOrig.panX + dxScreen;
    state.panY = dragOrig.panY + dyScreen;
    applyStageTransform();
  } else if (dragMode === "move" && state.selectedIndex !== null && state.doc) {
    const el = state.doc.elements[state.selectedIndex];
    const ddx = Math.round(dxScreen / state.zoom);
    const ddy = Math.round(dyScreen / state.zoom);
    el.x = dragOrig.elX + ddx;
    el.y = dragOrig.elY + ddy;
    const [bx, by, bw, bh] = state.bounds[state.selectedIndex];
    const deltaX = el.x - dragOrig.elX;
    const deltaY = el.y - dragOrig.elY;
    selectionBox.style.left = `${bx + deltaX - 4}px`;
    selectionBox.style.top = `${by + deltaY - 4}px`;
    selectionBox.style.width = `${Math.max(8, bw + 8)}px`;
    selectionBox.style.height = `${Math.max(8, bh + 8)}px`;
    selectionBadge.textContent = `${el.raw_id} (${el.x}, ${el.y})`;
  } else if (dragMode === "resize" && state.selectedIndex !== null && state.doc) {
    const el = state.doc.elements[state.selectedIndex];
    const ddx = dxScreen / state.zoom;
    const ddy = dyScreen / state.zoom;
    if (el.kind === "Line") {
      el.line_length = Math.max(10, Math.round(dragOrig.l + (el.line_vertical ? ddy : ddx)));
    } else if (el.kind === "Box") {
      el.line_length = Math.max(10, Math.round(dragOrig.l + ddx));
      el.h_mult = Math.max(10, Math.round(dragOrig.h + ddy));
    } else if (el.kind === "Barcode") {
      if (el.barcode_type === "QR") {
        const size = Math.max(25, Math.max(dragOrig.baseW + ddx, dragOrig.baseH + ddy));
        el.barcode_module = Math.max(1, Math.min(30, Math.round(size / 25)));
      } else {
        el.barcode_height = Math.max(15, Math.round(dragOrig.bh + ddy));
        el.barcode_module = Math.max(1, Math.min(20, Math.round(dragOrig.bm + ddx / 30)));
      }
    } else {
      el.w_mult = Math.max(1, Math.min(20, Math.round((dragOrig.baseW * dragOrig.w + ddx) / dragOrig.baseW)));
      el.h_mult = Math.max(1, Math.min(20, Math.round((dragOrig.baseH * dragOrig.h + ddy) / dragOrig.baseH)));
    }
    state.isDirty = true;
    rebuildPreview();
  }
});

window.addEventListener("mouseup", () => {
  if (dragMode === "move" && state.selectedIndex !== null && state.doc) {
    const el = state.doc.elements[state.selectedIndex];
    if (el.x === dragOrig.elX && el.y === dragOrig.elY) {
      state.undoStack.pop();
    } else {
      state.isDirty = true;
      updateWindowTitle();
      rebuildPreview();
      setStatus(`Dipindahkan ${el.raw_id} ke (${el.x}, ${el.y})`);
    }
  } else if (dragMode === "resize") {
    updateWindowTitle();
    rebuildPreview();
  }
  dragMode = null;
});

viewport.addEventListener("wheel", (e) => {
  e.preventDefault();
  const rect = viewport.getBoundingClientRect();
  const mx = e.clientX - rect.left;
  const my = e.clientY - rect.top;
  const factor = e.deltaY < 0 ? 1.12 : 0.89;
  const nextZoom = Math.max(0.05, Math.min(5.0, state.zoom * factor));
  const scale = nextZoom / state.zoom;
  state.panX = mx - (mx - state.panX) * scale;
  state.panY = my - (my - state.panY) * scale;
  state.zoom = nextZoom;
  applyStageTransform();
}, { passive: false });

// Keyboard Shortcuts
window.addEventListener("keydown", (e) => {
  const tag = (document.activeElement?.tagName ?? "").toLowerCase();
  const isTyping = tag === "input" || tag === "textarea" || tag === "select";

  if (e.ctrlKey && e.key.toLowerCase() === "z" && !isTyping) {
    e.preventDefault();
    undo();
  } else if (e.ctrlKey && e.key.toLowerCase() === "y" && !isTyping) {
    e.preventDefault();
    redo();
  } else if (e.ctrlKey && e.key.toLowerCase() === "c" && !isTyping && state.selectedIndex !== null && state.doc) {
    e.preventDefault();
    state.clipboard = cloneDoc(state.doc).elements[state.selectedIndex];
    updateToolbarButtons();
    setStatus(`Disalin: ${state.clipboard.raw_id}`);
  } else if (e.ctrlKey && e.key.toLowerCase() === "v" && !isTyping && state.clipboard) {
    e.preventDefault();
    duplicateOrPaste(state.clipboard, "Ditempel");
  } else if (e.key === "Delete" && !isTyping && state.selectedIndex !== null) {
    e.preventDefault();
    deleteSelected();
  } else if (!isTyping && state.selectedIndex !== null && state.doc && ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(e.key)) {
    e.preventDefault();
    const step = e.shiftKey ? 10 : 1;
    pushUndo();
    const el = state.doc.elements[state.selectedIndex];
    if (e.key === "ArrowLeft") el.x -= step;
    if (e.key === "ArrowRight") el.x += step;
    if (e.key === "ArrowUp") el.y -= step;
    if (e.key === "ArrowDown") el.y += step;
    state.isDirty = true;
    updateWindowTitle();
    rebuildPreview();
  }
});

// Wire Toolbar & Controls
tabGallery.addEventListener("click", () => switchMode("gallery"));
tabEditor.addEventListener("click", () => switchMode("editor"));

btnOpen.addEventListener("click", async () => {
  const loaded = await invoke<LoadedLabel | null>("pick_and_open_label");
  if (loaded) {
    applyLoadedLabel(loaded);
    switchMode("editor");
  }
});

btnSaveTax.addEventListener("click", async () => {
  if (!state.doc) return;
  const base = state.currentFileName.replace(/\.(tax|dax)$/i, "") || "label";
  const saved = await invoke<string | null>("save_tax_dialog", { doc: state.doc, defaultName: `${base}.TAX` });
  if (saved) {
    state.isDirty = false;
    updateWindowTitle();
    setStatus(`.TAX disimpan ke: ${saved}`);
  }
});

btnSaveDax.addEventListener("click", async () => {
  if (!state.doc) return;
  const base = state.currentFileName.replace(/\.(tax|dax)$/i, "") || "label";
  const saved = await invoke<string | null>("save_dax_dialog", { doc: state.doc, defaultName: `${base}.DAX` });
  if (saved) {
    state.isDirty = false;
    updateWindowTitle();
    setStatus(`.DAX disimpan ke: ${saved}`);
  }
});

btnSaveSvg.addEventListener("click", async () => {
  if (!state.svgCache) return;
  const base = state.currentFileName.replace(/\.(tax|dax)$/i, "") || "preview";
  const saved = await invoke<string | null>("save_svg_dialog", { svg: state.svgCache, defaultName: `${base}.svg` });
  if (saved) setStatus(`SVG disimpan ke: ${saved}`);
});

btnSavePng.addEventListener("click", async () => {
  if (!state.svgCache) return;
  const base = state.currentFileName.replace(/\.(tax|dax)$/i, "") || "preview";
  const saved = await invoke<string | null>("save_png_dialog", { svg: state.svgCache, defaultName: `${base}.png` });
  if (saved) setStatus(`PNG disimpan ke: ${saved}`);
});

btnUndo.addEventListener("click", undo);
btnRedo.addEventListener("click", redo);
btnCopy.addEventListener("click", () => {
  if (state.selectedIndex !== null && state.doc) {
    state.clipboard = cloneDoc(state.doc).elements[state.selectedIndex];
    updateToolbarButtons();
    setStatus(`Disalin: ${state.clipboard.raw_id}`);
  }
});
btnPaste.addEventListener("click", () => {
  if (state.clipboard) duplicateOrPaste(state.clipboard, "Ditempel");
});

chkVarNames.addEventListener("change", () => {
  state.showVarNames = chkVarNames.checked;
  rebuildPreview();
});
chkAutoCanvas.addEventListener("change", () => {
  state.autoCanvas = chkAutoCanvas.checked;
  rebuildPreview(true);
});
chkRuler.addEventListener("change", () => {
  state.showRuler = chkRuler.checked;
  canvasArea.classList.toggle("no-ruler", !state.showRuler);
  drawRulers();
});

btnZoomOut.addEventListener("click", () => {
  state.zoom = Math.max(0.05, state.zoom * 0.8);
  applyStageTransform();
});
btnZoomIn.addEventListener("click", () => {
  state.zoom = Math.min(5.0, state.zoom * 1.25);
  applyStageTransform();
});
btnFit.addEventListener("click", fitToWindow);

document.getElementById("tool-select")!.addEventListener("click", () => {
  state.selectedIndex = null;
  renderLayers();
  renderProperties();
  updateSelectionOverlay();
});
document.getElementById("tool-text")!.addEventListener("click", () => addNewElement("text"));
document.getElementById("tool-line")!.addEventListener("click", () => addNewElement("line"));
document.getElementById("tool-box")!.addEventListener("click", () => addNewElement("box"));
document.getElementById("tool-barcode")!.addEventListener("click", () => addNewElement("barcode"));
document.getElementById("tool-qr")!.addEventListener("click", () => addNewElement("qr"));
document.getElementById("tool-graphic")!.addEventListener("click", importGraphic);

layerSearch.addEventListener("input", () => {
  state.layerSearch = layerSearch.value;
  renderLayers();
});

layerFilters.addEventListener("click", (e) => {
  const btn = (e.target as HTMLElement).closest(".chip") as HTMLElement | null;
  if (!btn) return;
  layerFilters.querySelectorAll(".chip").forEach((c) => c.classList.remove("active"));
  btn.classList.add("active");
  state.layerFilter = (btn.dataset.filter as "All" | ElementKind) ?? "All";
  renderLayers();
});

btnDuplicate.addEventListener("click", () => {
  if (state.selectedIndex !== null && state.doc) {
    duplicateOrPaste(state.doc.elements[state.selectedIndex], "Diduplikasi");
  }
});
btnDelete.addEventListener("click", deleteSelected);

function applyManualCanvas(wDots: number, hDots: number) {
  state.autoCanvas = false;
  chkAutoCanvas.checked = false;
  state.canvasWidth = Math.max(100, Math.min(10000, Math.round(wDots)));
  state.canvasHeight = Math.max(100, Math.min(10000, Math.round(hDots)));
  syncCanvasInputs();
  rebuildPreview();
}

cvWDots.addEventListener("change", () => applyManualCanvas(Number(cvWDots.value), state.canvasHeight));
cvWMm.addEventListener("change", () => applyManualCanvas(Number(cvWMm.value) * DOTS_PER_MM, state.canvasHeight));
cvHDots.addEventListener("change", () => applyManualCanvas(state.canvasWidth, Number(cvHDots.value)));
cvHMm.addEventListener("change", () => applyManualCanvas(state.canvasWidth, Number(cvHMm.value) * DOTS_PER_MM));

btnPickFolder.addEventListener("click", async () => {
  const dir = await invoke<string | null>("pick_gallery_folder");
  if (dir) {
    state.galleryDir = dir;
    galleryFolderPath.textContent = dir;
    await scanGallery();
  }
});
btnRefreshGallery.addEventListener("click", scanGallery);
gallerySearch.addEventListener("input", () => {
  state.gallerySearch = gallerySearch.value;
  renderGalleryGrid();
});
chkGalleryVars.addEventListener("change", () => {
  state.galleryShowVars = chkGalleryVars.checked;
  thumbCache.clear();
  renderGalleryGrid();
});

window.addEventListener("resize", () => drawRulers());

// Startup: auto-load CLI file if provided, else default to Gallery or Editor
(async () => {
  getCurrentWebview()
    .onDragDropEvent((event) => {
      if (event.payload.type === "drop" && event.payload.paths.length > 0) {
        openLabelFile(event.payload.paths[0]);
      }
    })
    .catch(() => {});

  const initialFile = await invoke<string | null>("get_initial_file");
  if (initialFile) {
    await openLabelFile(initialFile);
  } else {
    // Open Gallery by default if the server backup folder exists!
    const defaultDir = await invoke<string>("get_default_gallery_dir");
    if (defaultDir) {
      state.galleryDir = defaultDir;
      switchMode("gallery");
    }
  }
})();
