# Refactoring Lanjutan — Pemecahan Modul Jumbo `crates/` & Waktu Kompilasi

> **SCOPE DOKUMEN INI = SESI "REFACTORING RUST" SAJA.** Hanya perubahan Rust:
> `crates/`, `src/`, `apps/zerocode`, `apps/tauri-2` (Rust), `Cargo.toml`/`Cargo.lock`.
> Jalur RF-A (pecah file), RF-B (pecah crate), RF-M (build-speed).
>
> **BUKAN di sini** (ada di `README.md` + `task-breakdown.md` = SSOT Migration, sesi berbeda):
> lane engine Go (`engine/**`), lane web-2 (`web-2/**`), DiskStore/Quartermaster/seed/apiClient.
> Dua dokumen sengaja dipisah agar sesi Rust dan sesi SSOT **tidak redundan / tidak bentrok writer.**
> Satu-satunya titik singgung: RF-A7 menyentuh `clawcrew-gateway` yang juga disebut Lane D (Go→Rust
> SystemGateway) di task-breakdown — Lane D menambah RPC handler, RF-A7 hanya memecah file; koordinasikan
> bila dikerjakan bersamaan.

> Masalah Owner: file-file raksasa (>10k baris) di `crates/` sulit di-maintain, memusingkan, dan menghambat kompilasi.
> Semua angka di bawah **diukur** (`Get-Content | Measure-Object -Line`) pada 2026-10-01, bukan tebakan.
> Status: `[ ]` belum · `[~] @nama` sedang · `[x]` selesai+verifikasi.

---

## 1. Fakta terukur (dasar keputusan)

- **Total Rust: ~1.004.896 LOC** di 1.036 file.
- **16 file > 10.000 baris**, 19 file 5k–10k, 73 file 2k–5k.

### LOC per crate (penghambat kompilasi utama)

| Crate | LOC | Files | Catatan |
|-------|-----|-------|---------|
| `clawcrew-runtime` | 288.524 | 310 | **Terberat** — satu crate = recompile mahal tiap edit |
| `clawcrew-channels` | 197.818 | 93 | 1 crate memuat SEMUA channel (telegram, slack, matrix, discord, wechat, lark…) |
| `clawcrew-tools` | 85.443 | 96 | |
| `clawcrew-config` | 85.058 | 42 | `schema.rs` 25k + `schema/tests.rs` 18k |
| `clawcrew-providers` | 84.251 | 46 | anthropic/compatible/reliable tiap ~11k |
| `apps/zerocode` | 77.362 | 50 | `chat.rs` 20k; tidak compile (S8) |

### File jumbo (>10k) — target pemecahan

| Baris | Jenis | File |
|-------|-------|------|
| 31.137 | **TEST** | `clawcrew-channels/src/orchestrator/tests.rs` |
| 25.212 | SRC | `clawcrew-config/src/schema.rs` |
| 20.596 | SRC | `apps/zerocode/src/chat.rs` |
| 19.214 | SRC | `clawcrew-runtime/src/agent/loop_.rs` |
| 18.840 | **TEST** | `clawcrew-config/src/schema/tests.rs` |
| 18.214 | SRC | `clawcrew-runtime/src/rpc/dispatch.rs` |
| 17.529 | SRC | `clawcrew-channels/src/orchestrator/mod.rs` |
| 15.184 | SRC | `clawcrew-runtime/src/agent/agent.rs` |
| 15.175 | SRC | `clawcrew-runtime/src/sop/engine.rs` |
| 13.539 | SRC | `clawcrew-runtime/src/tools/delegate.rs` |
| 13.295 | SRC | `src/main.rs` (root bin) |
| 11.587 | SRC | `clawcrew-gateway/src/lib.rs` |
| 11.547 / 11.467 | SRC | `clawcrew-providers/src/{reliable,compatible}.rs` |
| 11.349 | SRC | `clawcrew-channels/src/matrix.rs` |

**Insight kunci:** 2 dari 6 file terbesar adalah **file test** (31k + 18k = ~50k baris). Memisahnya **murah & aman** (tidak mengubah API) tapi langsung memangkas beban incremental compile unit test. Ini quick-win Sprint 0.

---

## 2. Mengapa ini menghambat kompilasi (akar masalah)

Rust meng-compile **per crate sebagai unit (codegen unit)**. Konsekuensi:
1. **Crate gemuk = recompile gemuk.** Edit 1 baris di `clawcrew-runtime` → seluruh 288k LOC crate itu dianalisis ulang. Memecah file dalam crate yang sama **tidak** memperbaiki ini sendirian.
2. **Yang benar-benar menurunkan waktu build: memecah crate besar jadi crate-crate kecil** (parallel codegen + caching per crate). Memecah file hanya memperbaiki *maintainability* + sebagian incremental.
3. **File 10k+ baris** memberatkan analisis borrow-checker/type-inference per fungsi dan bikin `rustc` lambat di file itu, plus editor/rust-analyzer tersendat.

Maka plan dibagi dua jalur: **(A) pecah file** (maintainability, cepat, aman) dan **(B) pecah crate** (compile-time, lebih berat, berdampak besar).

---

## 2b. Tracker status (per 2026-10-01) — baca ini dulu

> Satu-satunya papan skor sesi Rust. Perbarui saat sebuah RF selesai + ter-commit.

| RF | Ringkas | Status |
|----|---------|--------|
| RF-A0 | Split test jumbo (orchestrator/telegram/schema → `tests.rs` sibling) | ✅ `420c9dee` |
| RF-B1 | signal + bluesky + core → `galleon-channel-*` (proof-of-pattern) | ✅ `3893a58a`,`9c8c61ee` |
| RF-M2 | sccache + nextest + profil dev (`.cargo/config.toml`, `.config/nextest.toml`) | ⬛ sebagian (sisa: cranelift dev) |
| RF-A1 | `clawcrew-config/src/schema.rs` (25k) → submodul by-domain | ⬜ |
| RF-A2 | `runtime/agent/{loop_.rs 19k, agent.rs 15k}` → submodul | ⬜ |
| RF-A3 | `runtime/{rpc/dispatch.rs 18k, sop/engine.rs 15k, tools/delegate.rs 13k}` | ⬜ |
| RF-A4 | `channels/orchestrator/mod.rs` (17.5k) + `matrix.rs` (11.3k) → submodul | ⬜ |
| RF-A5 | `apps/zerocode/src/chat.rs` (20.6k) → submodul | ⬜ |
| RF-A6 | `src/main.rs` (13.3k) → modul, `main.rs` tinggal wiring | ⬜ |
| RF-A7 | `gateway/src/lib.rs` (11.6k) + `providers/{reliable,compatible}.rs` | ⬜ |
| RF-B1+ | channel Tier B lain: matrix, whatsapp-web, wechat, mattermost, lark | ⬜ |
| RF-B0 | peta dependency intra-crate runtime/channels (prasyarat B2) | ⬜ |
| RF-B2 | pecah `clawcrew-runtime` (288k) → sub-crate (butuh RF-B0) | ⬜ |
| RF-B3 | pecah `clawcrew-providers` (84k) per-vendor | ⬜ |
| RF-M1 | baseline `cargo build --timings` | ⬜ |
| RF-M3 | CI gate soft: file baru >2k baris → warning | ⬜ |
| RF-M4 | ukur ulang `--timings` vs baseline tiap RF-B | ⬜ |

Catatan: angka LOC file di tabel ini sudah memperhitungkan RF-A0 (mis. `orchestrator/mod.rs` kini 17.5k setelah 31k test dipisah). Build/test ditahan sampai Owner perintah.

## 3. Rencana — dua jalur, dikerjakan berurutan per crate

### JALUR A — Pecah file jumbo (aman, tanpa ubah API publik)
Teknik Rust: ubah `foo.rs` → folder `foo/` dengan `mod.rs` + submodul, pakai `pub(crate) use` re-export supaya path pemanggil tidak berubah. Satu file ≤ ~800 baris sebagai target.

- [x] **RF-A0 (quick-win)** — Pisah file **test** jumbo jadi file `tests.rs` sibling (teknik `foo.rs` + `foo/tests.rs`, induk `mod tests;`). **Selesai (commit `420c9dee`)**: `orchestrator/mod.rs` −33.7k → `orchestrator/tests.rs` (620 test); `telegram.rs` −15.4k → `telegram/tests.rs` (365 test); `schema.rs` −20.7k → `schema/tests.rs` (690 test). Pure-move, test count terjaga.
  - acceptance: ✅ pure-move (`git diff` simetris); induk re-declare submodul; path tak berubah. Build/test ditahan sampai Owner perintah.
- [ ] **RF-A1** — `clawcrew-config/src/schema.rs` (25k) → `schema/{mod.rs, agents.rs, channels.rs, providers.rs, policy.rs, …}` by domain config.
  - acceptance: `cargo check -p clawcrew-config` hijau; API `schema::*` tidak berubah (re-export).
- [ ] **RF-A2** — `clawcrew-runtime/src/agent/loop_.rs` (19k) + `agent/agent.rs` (15k) → submodul by tanggung jawab (state, step, tool-dispatch, streaming).
  - acceptance: `cargo check -p clawcrew-runtime` hijau; test agent lulus.
- [ ] **RF-A3** — `clawcrew-runtime/src/rpc/dispatch.rs` (18k) & `sop/engine.rs` (15k) & `tools/delegate.rs` (13k) → submodul.
  - acceptance: idem per file.
- [ ] **RF-A4** — `clawcrew-channels/src/orchestrator/mod.rs` (17k) & `matrix.rs` (11k) → submodul.
- [ ] **RF-A5** — `apps/zerocode/src/chat.rs` (20k) → submodul (**setelah** S8/A1 build zerocode hijau).
- [ ] **RF-A6** — `src/main.rs` (13k, root bin) → pindah logika ke modul `src/<area>/`; `main.rs` tinggal wiring.
- [ ] **RF-A7** — `clawcrew-gateway/src/lib.rs` (11k) & `providers/{reliable,compatible}.rs` → submodul.

### JALUR B — Pecah crate besar jadi crate kecil (menurunkan compile-time)
Prioritas: `clawcrew-runtime` (288k) dan `clawcrew-channels` (197k) — dua ini yang paling membebani.

- [~] **RF-B1** — `clawcrew-channels` → crate per-channel `galleon-channel-<name>` + `galleon-channel-core` (trait/policy/approval bersama).
  - rasional: tiap channel independen; edit telegram tidak perlu recompile matrix.
  - **Selesai (proof-of-pattern, commit `3893a58a` + `9c8c61ee`):** `galleon-channel-core` (allowlist peer-policy re-export dari `clawcrew_config::schema` SSOT + approval helper i18n-free, TANPA drag `clawcrew-runtime`); `galleon-channel-signal` (signal-cli JSON-RPC/SSE); `galleon-channel-bluesky` (AT Protocol). Monolith re-export `pub use galleon_channel_{signal,bluesky} as {signal,bluesky}`; feature `channel-{signal,bluesky} = ["dep:…"]`.
  - **Terbuka (ikuti pola di atas):** `-matrix` (11.3k), `-whatsapp-web` (7.4k), `-wechat` (6.5k), `-mattermost` (5.9k), `-lark` (7.2k) [Tier B]; channel Tier C via WASI plugin (lihat ADR-001). Telegram/discord/slack = Tier A, TETAP embed (jangan dipindah).
  - acceptance: workspace build hijau; `cargo build -p galleon-channel-<name>` hanya meng-compile channel+core; nama baru `galleon-*`.
- [ ] **RF-B2** — Pecah `clawcrew-runtime` (288k) jadi beberapa crate by domain: `-runtime-agent`, `-runtime-sop`, `-runtime-rpc`, `-runtime-tools`, `-runtime-daemon`, sisakan `-runtime` sebagai fasad tipis.
  - **PRASYARAT:** petakan dependency internal dulu (lihat RF-B0) agar tidak ada import siklik.
  - acceptance: tidak ada cyclic crate dep; `cargo build` paralel per crate; incremental edit di `agent` tidak recompile `channels`.
- [ ] **RF-B0 (prasyarat B)** — Buat peta dependency intra-crate `clawcrew-runtime` & `clawcrew-channels` (pakai graphify `path`/`query`) untuk menentukan batas crate yang bebas-siklus.
  - acceptance: diagram modul→modul; daftar edge yang harus diputus sebelum split.
- [ ] **RF-B3** — Evaluasi `clawcrew-providers` (84k): pecah per vendor (`-provider-anthropic`, `-provider-openai-compatible`, `-provider-core`) bila RF-B1/B2 terbukti menurunkan build time.

---

## 4. Guardrail waktu kompilasi (ukur, jangan tebak)

- [ ] **RF-M1** — Baseline sebelum refactor: `cargo build --timings` + simpan HTML; catat waktu clean build & incremental (edit 1 file di runtime).
- [ ] **RF-M2** — Set `codegen-units` & profil dev untuk iterasi cepat; evaluasi `cranelift` backend dev bila cocok.
- [ ] **RF-M3** — Tambah CI gate soft: file baru > 2.000 baris → warning review (cegah regresi jumbo). Lihat `scripts/ci/`.
- [ ] **RF-M4** — Setelah tiap RF-B, bandingkan `--timings` vs baseline RF-M1; terima hanya bila incremental turun nyata.

---

## 5. Urutan & paralelisasi

```
Sprint 0 (quick-win, aman):   RF-A0  | RF-M1 (baseline)
Sprint 1 (file split, paralel): RF-A1 | RF-A2 | RF-A4 | RF-A7   (file beda = tanpa bentrok)
Sprint 2 (file split):         RF-A3 | RF-A6 | RF-A5(after S8)
Sprint 3 (crate split):        RF-B0 → RF-B1  (channels dulu: paling mudah, batas jelas)
Sprint 4 (crate split berat):  RF-B2 (runtime, butuh RF-B0) | RF-B3 (providers)
Sprint 5:                      RF-M3 CI gate | RF-M4 ukur ulang
```

**Aturan paralel:** JALUR A antar-file berbeda = aman diparalelkan (tidak ada writer bentrok). JALUR B **serialize per crate** (split crate menyentuh `Cargo.toml` workspace + banyak import) — satu crate split = satu PR, jangan dua split crate sekaligus.

## 6. Prinsip (hemat & aman)

- **Pecah tanpa mengubah perilaku.** Setiap RF-A/B harus pure-move: `git diff` idealnya hanya pindah baris + re-export, nol perubahan logika. Verifikasi: test suite crate hijau sebelum & sesudah.
- **Re-export jaga API** (`pub use`) → pemanggil tidak ikut berubah → PR tetap kecil.
- **Satu file/crate = satu PR.** Jangan campur split dengan perbaikan logika.
- **Nama baru `galleon-*`** (branding), kode `clawcrew-*` lama tidak di-rename massal kecuali saat crate-nya memang dipecah.
- Prioritaskan dampak: `runtime` + `channels` dulu (486k dari ~1jt LOC).

---

## 7. Pelajaran build-speed (update 2026-10-01 — dari sesi eksekusi)

Konteks: compile start awal + test lambat sudah jadi hambatan nyata Owner. Yang berubah dari plan awal:

### Prioritas NAIK — ini bukan "nanti"
Terbukti di sesi ini: nested `cargo build` wasm dalam 1 test = 84s; crate besar + memory tight = sumber waktu terbuang utama. **RF-B2 (`clawcrew-runtime` 288k) + RF-B1 (`clawcrew-channels` 197k) adalah satu-satunya penurunan inner-loop recompile yang NYATA** — flag/profile hanya memangkas konstanta. Naikkan dari Sprint 3/4 → jalur prioritas, sejajar SSOT migration (README.md).

### Sudah dikonfigurasi (jangan ulang)
- sccache aktif (`.cargo/config.toml`), nextest (`.config/nextest.toml`). RF-M2 sebagian selesai. Sisa RF-M2: evaluasi `debug="line-tables-only"` + `[profile.dev.package."*"] opt-level=2` + cranelift dev.
- Standar global: `~/.kiro/steering/rust-build-speed.md` (jangan duplikasi ke sini).

### Aturan eksekusi AMAN (wajib untuk tiap task RF di atas)
Dari insiden subagent "stuck" 20+ menit:
- Build/test berat (wasm, nested cargo, `cargo test`, graphify repo besar) → **work session terpisah**, bukan inline.
- **Cek `resource_status` dulu.** Memory tight → compile-only (`cargo test --no-run -p <crate>` / `cargo check -p <crate>`), bukan test penuh.
- **Serialkan** — jangan paralel >1 build berat; jangan bentrok writer `Cargo.toml` (JALUR B sudah serialize, pertahankan).
- Subagent lama tak respons ≠ stuck → **cek disk + transkrip SEBELUM kill**.

### Sinergi dengan ADR-001 (channel plugin)
RF-B1 (pecah `clawcrew-channels` per-channel) kini punya arah lebih tajam: lihat `ADR-001-channel-plugin-distribution.md`. Channel Tier B → feature-crate `galleon-channel-<name>`; Tier C → WASI plugin. Pilot `channel-nostr-pilot` **sudah terbukti build + load hijau** (test e2e `1 passed`) — pola plugin tervalidasi, siap jadi template RF-B1 lane plugin.

