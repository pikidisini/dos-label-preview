# STATUS.md — Status Proyek Saat Ini

- **Versi Stabil Tersimpan:** `v1.0.0` (tag di branch `main`, commit `9033115`)
- **Branch Aktif:** `feat/editor`
- **Fase Saat Ini:** Fase 3 — Selesai (Siap diuji coba user)
- **Hasil Verifikasi Terakhir:**
  - `cargo test`: **5 passed; 0 failed** (`parser::tests::*`, `writer::tests::*`).
  - `cargo build --release`: **Berhasil** menghasilkan `target\release\dos_label_preview.exe`.
- **Catatan Rollback:** Jika ingin kembali ke versi viewer awal tanpa editor, jalankan `git checkout main` (atau `git checkout v1.0.0`).
