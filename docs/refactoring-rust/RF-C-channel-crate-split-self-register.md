# RF-C — Lanjutan RF-B1/Tier-C: Channel Crate-Split, WASI Self-Register, Test-Split

- **Status:** Draft — menunggu Principle gate di PR pertama, bukan approval user (lanjutan eksekusi ADR-001 yang sudah proposed)
- **Author:** squad-lead
- **Tanggal:** 2026-10-03
- **Scope:** `crates/clawcrew-channels/src/`, `crates/galleon-channel-*/`, `crates/clawcrew-plugins/` (host-side wiring saja, bukan WIT contract ulang)
- **Tidak di-scope:** `clawcrew-config/src/schema.rs` (25k baris, SSOT config — RF-A terpisah), ADR-001 sign-off formal (asumsi: lanjutkan mapping yang sudah ada, laporkan penyimpangan sebagai temuan bukan blocker)

## Konteks

ADR-001 (`docs/refactoring-phase2/ADR-001-channel-plugin-distribution.md`) sudah memetakan 35 channel ke 3 tier. Baru 2 dari ~26 kandidat Tier B/C yang dieksekusi (signal, bluesky — keduanya Tier B feature-crate; bluesky secara ADR seharusnya Tier C tapi sudah native, dicatat sebagai deviasi diterima — lihat `## Catatan Deviasi`). RF-C melanjutkan sisa migrasi + menuntaskan test-split yang masih campur di 30+ file channel.

Tiga pekerjaan paralel, independen satu sama lain (tidak saling blocking):

1. **RF-C1 — Tier B crate-split** (file-move, compile-in, menurunkan compile unit)
2. **RF-C2 — Tier C WASI self-register** (plugin hot-load, menurunkan binary size + true "self-register")
3. **RF-C3 — Test-split** (file-split murni, menaikkan maintainability, tidak pengaruhi build time)

## Catatan Deviasi (bukan blocker, dicatat untuk Principle)

`galleon-channel-bluesky` dieksekusi sebagai Tier B (compiled feature-crate), padahal ADR-001 memetakannya Tier C (polling/social, tidak butuh socket native). RF-C **tidak membongkar bluesky** — biaya rollback lebih mahal dari manfaat saat ini, dan ADR sendiri bilang "jangan migrasi sekaligus". RF-C2 memperlakukan bluesky sebagai **sudah selesai** dan tidak dimasukkan ke daftar porting Tier C. Principle cukup catat ini di review log, tidak perlu di-gate.

## Koreksi Tier (2026-10-03, ditemukan saat eksekusi RF-C1 #1)

**`matrix` DIKELUARKAN dari RF-C1.** Investigasi saat porting menemukan `matrix` berbagi governance primitive (`PendingApproval`/`resolve_pending_approval`/`build_approve_deny_approval_prompt`) dengan `slack`+`telegram` di `clawcrew-channels/src/util.rs` — approval-queue CORE, bukan channel-specific logic. Preseden KiroCrew (`src/kiro_crew/hooks.py` PreToolUse Gate dipanggil semua channel, tidak diduplikasi/dipinjam antar-channel per-channel) mengonfirmasi: sharing governance primitive dengan Tier A lain adalah SINYAL keanggotaan Tier A, bukan masalah teknis yang perlu diselesaikan dengan promote-to-core.

ADR-001 sudah direvisi: `matrix` pindah dari Tier B ke Tier A. RF-C1 prioritas list di bawah diperbarui — matrix dicoret, diganti channel yang benar-benar terisolasi (tidak muncul sama sekali di `util.rs` shared functions).

**Verifikasi isolasi dilakukan sebelum eksekusi berikutnya:** grep `clawcrew-channels/src/util.rs` untuk `feature = "channel-<name>"` — channel yang TIDAK muncul sama sekali (`imessage`, `irc`, `amqp`, `nextcloud_talk`) aman tanpa pengecualian. Channel yang muncul tapi hanya memakai HELPER TEKNIS generik (`ensure_success`, `read_response_body_limited` — dipakai `wechat`/`qq`/`dingtalk`/`line`/`mochat`/`twitter`/`wecom`/`discord`, bukan governance primitive) tetap AMAN untuk Tier B/C: helper itu boleh diduplikasi kecil atau dipromosikan ke `galleon_channel_core` sebagai utilitas HTTP, tidak membawa behavior-coupling seperti approval-queue.

---

## RF-C1 — Tier B: Crate-split channel compiled-in

**Pola wajib diikuti (sudah proven 2x, jangan improvisasi):**
1. Buat `crates/galleon-channel-<name>/` dengan `Cargo.toml` + `src/lib.rs`.
2. Port body dari `clawcrew-channels/src/<name>.rs` apa adanya — pure-move, bukan rewrite. Pakai `galleon-channel-core::allowlist` untuk peer-policy (jangan reimplement).
3. Dependency yang menarik `clawcrew-runtime` (i18n, structured log) → **dokumentasikan sebagai ceiling** (`// ponytail: ...`), jangan porting penuh kalau nambah dependency berat. Lihat pola `galleon-channel-bluesky/src/lib.rs` baris 14-23 untuk contoh ceiling yang sudah diterima.
4. Di `clawcrew-channels/src/lib.rs`: ganti `pub mod <name>;` jadi `pub use galleon_channel_<name> as <name>;` di balik `#[cfg(feature = "channel-<name>")]` yang sama.
5. Tambahkan ke `[workspace.dependencies]` root `Cargo.toml` + `members`.
6. **Hapus** `clawcrew-channels/src/<name>.rs` lama SETELAH re-export terbukti resolve (`cargo check -p clawcrew-channels --features channel-<name>`). Jangan biarkan dua implementasi hidup bersamaan (SSOT).
7. Satu writer per crate channel, serialkan — jangan paralel di crate yang sama (lesson existing).

**Urutan prioritas (berat kompilasi/besar file dulu):**

| # | Channel | Baris asal | Alasan prioritas |
|---|---|---|---|
| 1 | `imessage` | sedang (lihat crate asal) | **AMAN, nol-sharing** — tidak muncul di util.rs sama sekali, macOS-only native bridge, isolasi bersih. Prioritas #1 PENGGANTI matrix. |
| 2 | `irc`, `amqp`, `nextcloud_talk` | kecil-menengah | **AMAN, nol-sharing** — tidak muncul di util.rs. Persistent-connection, batch bareng karena pola serupa |
| 3 | `whatsapp` + `whatsapp_web` + `whatsapp_storage` | 3,212 + 7,392 + 2,234 | 3 file saling terkait, port sekaligus 1 crate `galleon-channel-whatsapp`. Cek dulu util.rs sebelum eksekusi — belum diverifikasi isolasinya |
| 4 | `wechat`, `qq` | 6,491 + 2,667 | Memakai `ensure_success`/`read_response_body_limited` dari util.rs — HELPER TEKNIS generik, bukan governance primitive, tetap AMAN untuk Tier B. Duplikasi kecil helper itu atau promosikan ke `galleon_channel_core` sebagai util HTTP |
| 5 | `mattermost` | 5,930 | WS + payload besar, test masih campur (12 blok). Cek util.rs dulu sebelum eksekusi |
| ~~6~~ | ~~`matrix`~~ | ~~5,129+6,219~~ | **DIKELUARKAN — pindah ke Tier A, lihat Koreksi Tier di atas** |
| 7 | voice-stack (`voice`, `tts`, `transcription`, `voice_call`, `voice_wake`) | 2,483 + 2,594 + lainnya | audio pipeline, satu crate `galleon-channel-voice` (5 modul jadi submodule, bukan 5 crate terpisah — hindari over-fragmentasi). Cek util.rs dulu |

**Wajib sebelum eksekusi tiap channel di tabel ini:** `grep 'feature = "channel-<name>"' crates/clawcrew-channels/src/util.rs` — kalau hasilnya kosong, aman langsung. Kalau muncul, baca fungsi apa yang dipakai: helper teknis generik (HTTP/parsing) = tetap aman; governance/approval primitive = STOP, lapor ke Lead untuk evaluasi ulang tier (seperti kasus matrix).

**Definition of Done per channel:** `cargo check -p galleon-channel-<name>` hijau, `cargo check -p clawcrew-channels --features channel-<name>` hijau, file lama terhapus, re-export terbukti resolve dari orchestrator, tidak ada `unwrap()`/`expect()` baru, 1 test smoke jalan.

---

## RF-C2 — Tier C: WASI plugin self-register

**Ini jawaban sesungguhnya untuk "self-register"**: bukan macro Rust compile-time, tapi plugin WASI yang mendaftar dirinya via `plugin-manifest.toml` + signature, di-hot-load oleh `clawcrew-plugins` runtime saat start — tanpa rebuild core.

**Pola wajib diikuti (template sudah proven 1x di `channel-nostr-pilot`):**
1. Copy struktur dari `crates/clawcrew-plugins/tests/fixtures/channel-nostr-pilot/` (bukan dari channel-fixture generik — nostr-pilot sudah terbukti e2e untuk kasus channel sungguhan).
2. Port logic polling/webhook dari `clawcrew-channels/src/<name>.rs` ke `wit_bindgen::generate!` + `export!` sesuai `wit/v0/channel.wit` world `channel-plugin`.
3. **Cek batas teknis dulu sebelum porting** (ADR-001 poin #5): kalau channel butuh HTTP outbound aktif (posting, bukan cuma terima webhook), **tahan** — `wasi:http` outbound belum dibuka untuk plugin. Tandai sebagai blocked, jangan paksa porting.
4. `plugin-manifest.toml`: isi `config_schema`, `x-secret` untuk kredensial, `permissions` sesuai kebutuhan channel (minimal necessary, jangan default permissive).
5. Signature: ikuti `clawcrew-plugins/src/signature.rs` — plugin pilot internal boleh unsigned untuk dev, tapi dokumentasikan bahwa produksi/marketplace wajib sign sebelum WIT dibekukan.
6. Setelah plugin e2e lulus → **hapus** modul lama di `clawcrew-channels/src/<name>.rs` dan cabut dari `Cargo.toml` features (bukan cuma di-cfg-out, benar-benar dihapus — SSOT).

**Urutan prioritas (webhook-only dulu, paling cocok inbound-first):**

| # | Channel | Alasan urutan |
|---|---|---|
| 1 | `line` | webhook-based murni, regional tapi volume user cukup besar — bukti nilai pola di channel non-niche |
| 2 | `lark`, `dingtalk`, `wecom` | sama-sama webhook regional CN/APAC, pola serupa, batch setelah `line` jadi template kedua |
| 3 | `notion`, `linq`, `gmail_push` | webhook/API, tidak regional-spesifik |
| 4 | `mochat`, `clawdtalk` | niche, volume rendah, kerjakan setelah yang bervolume |
| 5 | `twitter`, `reddit`, `twitch` | **cek dulu**: kalau fitur posting/reply butuh HTTP outbound aktif, tahan sampai `wasi:http` dibuka — jangan porting partial yang pincang |
| 6 | `nostr` (production, bukan pilot) | pilot sudah ada, tinggal naik dari fixture ke crate produksi + hapus `clawcrew-channels/src/nostr.rs` |
| 7 | `git` | polling, tapi cek dulu apakah butuh akses filesystem lokal di luar WASI sandbox — kalau ya, tahan di Tier B bukan C |

**Definition of Done per plugin:** e2e test lulus (seperti nostr-pilot), manifest valid (`config_schema` + `permissions` minimal), modul lama di `clawcrew-channels` terhapus, dicatat di WIT `@unstable` tracker bahwa 1 lagi kontrak terpakai (menuju keputusan bekukan WIT v0).

---

## RF-C3 — Test-split (file-split murni, tidak terkait compile time)

**Pola wajib (RF-A0, sudah proven 4x: matrix, orchestrator, slack, telegram):**
1. Identifikasi blok `#[cfg(test)] mod tests { ... }` di akhir file (rustfmt menjamin posisi + brace di column-0).
2. Pindah ke `<name>/tests.rs` sibling (kalau file jadi folder `<name>/mod.rs`) dengan `mod tests;` di parent.
3. Rebuild slice dari `git show HEAD:<file>` untuk hindari off-by-one kalau ada edit lain yang sudah mengubah file.
4. Verifikasi jumlah `#[test]`/`#[tokio::test]` di `tests.rs` baru = jumlah di file asal sebelum dipotong — bukti pure-move bukan test hilang.

**Catatan ketat (dari lesson tersimpan): file-split TIDAK mengurangi waktu compile.** Nilai RF-C3 murni maintainability manusia (navigasi, review diff lebih kecil, merge conflict lebih jarang). Jangan klaim ke Principle/user bahwa ini mempercepat build — kalau diklaim begitu Principle harus tolak klaimnya.

**Daftar file test-campur yang belum di-split (prioritas berdasar ukuran blok test):**

| # | File | Baris total | Blok `#[cfg(test)]` |
|---|---|---|---|
| 1 | `mattermost.rs` | 5,930 | 12 |
| 2 | `tts.rs` | 2,483 | 7 |
| 3 | `line.rs` | 2,689 | 5 |
| 4 | `whatsapp_web.rs` | 7,392 | 5 |
| 5 | `lark.rs` | 7,241 | 4 |
| 6 | `model_picker_delivery.rs` | — | 4 |
| 7 | `imessage.rs` | — | 3 |

Sisanya (1 blok test masing-masing: `discord/*`, `git/*`, channel kecil lain) — batch jadi 1 PR per 5-8 file sekaligus, karena perubahan mekanis identik dan review-nya cepat (bukan karena boleh di-`git add .`, tetap staged per file eksplisit).

**Catatan urutan dengan RF-C1/RF-C2:** kalau sebuah channel sudah dijadwalkan RF-C1/RF-C2 (misal `mattermost`, `line`, `whatsapp_web`), **jangan test-split dulu** — port langsung ke crate baru sekalian (test ikut pindah utuh ke crate baru, bukan di-split di tempat lalu dipindah lagi). Test-split RF-C3 hanya untuk channel yang **tetap tinggal** di `clawcrew-channels` dalam waktu dekat (Tier A, atau Tier B/C yang belum terjadwal RF-C1/RF-C2 batch berikutnya).

---

## Pembagian kerja

- **squad-backend**: eksekusi RF-C1 (crate-split) dan RF-C2 (WASI plugin), satu channel/plugin per PR, serialkan di crate yang sama.
- **squad-frontend / squad-qa**: tidak terlibat (scope ini murni Rust core).
- **Principle**: gate setiap PR ke `testing` — checklist di bawah.
- **squad-lead (saya)**: update artifact roadmap tiap channel kelar, refine spec kalau Backend lapor blocker teknis (misal `wasi:http` outbound ternyata dibutuhkan channel yang diasumsikan aman).

## Principle Review Checklist (brief terpisah, lihat pesan berikut)

Gate wajib per PR:
- [ ] SSOT: modul lama benar-benar **terhapus** (bukan di-cfg-out) setelah re-export terbukti resolve — tidak ada 2 implementasi hidup bersamaan.
- [ ] Tidak ada `unwrap()`/`expect()` baru di production path.
- [ ] RF-C1: `cargo check -p galleon-channel-<name>` DAN `cargo check -p clawcrew-channels --features channel-<name>` dua-duanya hijau (bukan cuma salah satu).
- [ ] RF-C2: manifest `permissions` minimal-necessary, bukan permissive default; tidak ada HTTP outbound aktif dipaksa lewat workaround kalau WIT belum izinkan.
- [ ] RF-C3: jumlah test sebelum/sesudah split sama (hitung manual, jangan percaya diff "terlihat aman").
- [ ] Satu PR = satu channel/plugin (atau batch kecil test-split eksplisit) — tolak PR yang mencampur RF-C1+RF-C3 di channel yang sama tanpa alasan di description.
- [ ] CI hijau sebelum APPROVE.

## Definition of Done RF-C (keseluruhan)

RF-C selesai ketika: semua channel Tier B termigrasi (RF-C1), semua channel Tier C non-blocked termigrasi (RF-C2), semua file sisa di `clawcrew-channels` yang TIDAK dijadwalkan pindah punya test terpisah (RF-C3), dan `cargo build --timings` baseline-vs-akhir menunjukkan penurunan (RF-M4, sudah ada di roadmap existing — RF-C tidak mengulang, hanya menyumbang data).
