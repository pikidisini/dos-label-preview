# STATUS.md — Status Proyek Saat Ini

- **Versi Stabil Tersimpan:** `v1.0.0` (tag di branch `main`, commit `9033115`)
- **Branch Aktif:** `feat/editor`
- **Fase Saat Ini:** Fase 4 — Selesai (Editor Lengkap + Tambah Elemen Baru)
- **Fitur yang Tersedia di `feat/editor`:**
  - Preview & Edit `.TAX` / `.DAX` (klik/drag posisi `X, Y`, edit skala `h/w/l`, font `c`, rotasi `0°/-90°/180°/90°`, teks statis/data DAX).
  - Tambah elemen baru: **`+ Teks`**, **`+ Garis`**, **`+ Kotak (W)`**, **`+ Barcode (Code 39)`**, **`+ QR Code`**, dan **`+ Grafis`** (import PNG/JPG/BMP otomatis ke 1-bit IPL 6-bit `G`+`u`).
  - **Duplikat** & **Hapus** elemen (`Delete`), **Undo / Redo** (`Ctrl+Z` / `Ctrl+Y`), serta ekspor **`.TAX`**, **`.DAX`**, dan **`.SVG`**.
- **Hasil Verifikasi Terakhir:**
  - `cargo test`: **9 passed; 0 failed**.
  - `cargo build --release`: **Berhasil** (`target\release\dos_label_preview.exe`).
