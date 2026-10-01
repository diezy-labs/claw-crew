# One-shot migration scripts (archived)

Skrip di folder ini adalah artefak migrasi **sekali-pakai** dari rebrand `clawcrew`→`galleon`
dan perbaikan ad-hoc. Dipindah dari root repo (task A3, `docs/refactoring-phase2`) agar root bersih.
Tidak dipakai CI/build; disimpan untuk jejak sejarah. Jangan jalankan ulang tanpa review —
sebagian menulis bulk ke sumber.

| Skrip | Tujuan asli (perkiraan dari nama) |
|-------|-----------------------------------|
| `rebrand.py`, `rebrand_icons.py` | *(masih di root — docs/branding, bukan scope A3)* |
| `fix_xtask.py` | Perbaikan xtask build helper |
| `fix_structs.py` | Perbaikan definisi struct saat migrasi |
| `fix_sidebar.py` | Perbaikan komponen sidebar UI |
| `fix_patch.py`, `patch.py`, `patch_ui.py` | Patch ad-hoc sumber/UI |
