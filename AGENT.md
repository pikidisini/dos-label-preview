# AGENT.md — Lead & Multi-Agent Engineering Guide

## Peran & Koordinasi
- **Lead Agent (Coordinator):** Bertanggung jawab atas arsitektur, pembagian tugas ke subagent spesialis, review kode, integrasi, serta verifikasi akhir (`cargo test` & `cargo build --release`).
- **Subagent Spesialis (dikerahkan saat fase implementasi):**
  1. **IPL Core & Writer Agent:** Menangani `model.rs`, `parser.rs`, dan modul `writer.rs` (round-trip `.TAX` / `.DAX`, preservasi command IPL printer, dan konversi gambar ke 6-bit UDC).
  2. **Canvas & UI Editor Agent:** Menangani interaksi kanvas di `main.rs` (seleksi elemen, *drag & drop* koordinat dot, snap/grid, panel inspector properti, dan Undo/Redo).
  3. **QA & Verification Agent:** Menjalankan `cargo test`, verifikasi round-trip parsing/writing terhadap file `.DAX`/`.TAX` nyata, serta memastikan tidak ada regresi pada rendering.

## Konteks Teknis Proyek
- **Direktori Proyek:** `C:\Users\fiqih\Documents\0000_TRST\Cline\00024_DOS_Label_Preview`
- **Tech Stack:** Rust (`eframe`/`egui` 0.29, `resvg` 0.44, `rfd` 0.15, `regex`, `once_cell`, `winresource`).
- **Branching & Rollback:**
  - `main` & tag `v1.0.0`: Versi standalone viewer awal (jangan diubah langsung).
  - `feat/editor`: Branch aktif pengembangan fitur editor.
- **Standar Verifikasi:** Setiap perubahan harus lolos `cargo test` dan `cargo build --release` sebelum dilaporkan selesai.
