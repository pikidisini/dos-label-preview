# TASK.md — Roadmap Pengembangan Editor Label IPL

## Fase 0: Pengumpulan Kebutuhan (`/grill-me`) — SELESAI
- [x] Alur kerja: Edit file `.TAX`/`.DAX` yang ada dan simpan kembali ke `.TAX` (template) atau `.DAX` (template + data).
- [x] Strategi IPL Writer: Pertahankan header/footer IPL asli, ID field (`H`/`B`/`L`/`U`), grafis (`G`/`u`), dan urutan baris agar 100% aman untuk printer Intermec & SAP.
- [x] Fitur editor: Kanvas interaktif (klik & drag posisi `oX,Y`), Daftar Elemen + Inspector Properti di panel kanan (`X`, `Y`, `h`, `w`, `l`, `c`, `f0..f3`, teks statis/variabel & nilai DAX), serta Undo/Redo (`Ctrl+Z` / `Ctrl+Y`).

## Fase 1: Fondasi Model, Parser & Lossless IPL Writer — SELESAI
- [x] Simpan raw lines / source line index pada `LabelDocument` dan `TaxElement` agar baris non-edit tetap utuh.
- [x] Implementasi `src/writer.rs`: update atribut IPL secara presisi (`oX,Y`, `fN`, `hN`, `wN`, `cN`, `lN`, `d3,...`, dan blok data DAX `<ESC>F"FIELD"<LF>VALUE`) serta ekspor `.TAX` dan `.DAX`.
- [x] Tambahkan helper estimasi bounding-box elemen (`element_bounds`) untuk seleksi & highlight di kanvas.
- [x] Unit test round-trip parsing -> edit -> write -> re-parse pada `parser.rs` & `writer.rs` (5/5 test lulus).

## Fase 2: Kanvas Interaktif & Panel Editor UI — SELESAI
- [x] Mode seleksi & drag elemen langsung di kanvas (`egui` pointer -> koordinat dot label), dengan bounding-box highlight pada elemen terpilih.
- [x] Panel kanan: **Daftar Elemen** (filter/klik pilih) + **Inspector Properti** (edit X/Y, skala `h`/`w`/`l`, font `c`, rotasi `f0..f3`, teks/data DAX).
- [x] Undo / Redo (`Ctrl+Z` / `Ctrl+Y`).
- [x] Tombol **Simpan .TAX** dan **Simpan .DAX** di toolbar.

## Fase 3: Verifikasi Akhir & Release — SELESAI
- [x] Jalankan `cargo test` (5 passed, 0 failed).
- [x] Jalankan `cargo build --release` dan verifikasi binary `dos_label_preview.exe`.
- [x] Update `STATUS.md` dan `README.md`.
