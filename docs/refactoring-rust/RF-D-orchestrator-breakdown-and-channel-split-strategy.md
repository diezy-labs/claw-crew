# RF-D — Strategi Lanjutan: Orchestrator Breakdown + Channel Crate-Split Terseleksi

- **Status:** Plan — BELUM dieksekusi, menunggu Backend availability
- **Author:** squad-lead
- **Tanggal:** 2026-10-03
- **Prasyarat:** RF-C1 #1 (imessage) di-merge dulu, Principle checklist RF-D (dokumen terpisah) di-sign-off
- **Scope:** `crates/clawcrew-channels/src/orchestrator/`, channel-channel Tier B/C tersisa yang belum terjadwal RF-C1/RF-C2

## Latar belakang (data real, dicek 2026-10-03)

Total `clawcrew-channels/src/`: 197.057 baris / 90 file. Setelah RF-C1 #1 (imessage) keluar, godfile terbesar BUKAN lagi channel individual — melainkan `orchestrator/` (mod.rs 18.568 + tests.rs 33.730 + acp_server.rs 10.426 = **62.724 baris, 31.8% dari total crate**). Audit test-coverage (2026-10-03) juga menemukan **153.499 baris di 39 file production lain** yang test-nya masih campur inline — scope test-split jauh lebih besar dari yang tercatat di RF-C3 sebelumnya. Orchestrator sendiri masih punya 27 blok test inline meski sibling tests.rs-nya sudah ada — RF-A0 sebelumnya cuma memindahkan sebagian.

**Kesimpulan strategis:** crate-split channel individual (RF-C1/RF-C2) dan file-split orchestrator (RF-D1) adalah DUA masalah berbeda yang harus diselesaikan PARALEL, bukan satu menggantikan yang lain. Memecah 20+ channel jadi crate kecil TANPA membongkar orchestrator hanya memindahkan beban, tidak menguranginya.

## Tiga prinsip evaluasi (WAJIB dicek sebelum eksekusi apapun di bawah)

1. **Compile-time benefit nyata hanya untuk channel BERAT** (dependency besar tidak dipakai channel lain: matrix-sdk, libsignal, dsb). Channel kecil (<2000 baris, dependency ringan) TIDAK layak crate-split — overhead `Cargo.toml`+publish-unit lebih mahal dari manfaatnya. Lihat `rust-build-speed.md`: "structural crate-split = only real inner-loop reduction."
2. **Governance-coupling check WAJIB** sebelum crate-split channel manapun: `grep 'feature = "channel-<name>"' crates/clawcrew-channels/src/util.rs`. Kosong = aman. Helper teknis generik (`ensure_success`, `read_response_body_limited`) = aman. Governance/approval primitive (`PendingApproval`, dst) = STOP, channel itu Tier A, bukan kandidat split (kasus matrix, ditemukan 2026-10-03).
3. **Transport-class check WAJIB** sebelum pilih native-crate vs WASI-plugin: channel inbound-only webhook/polling → WASI plugin kandidat. Channel butuh socket persistent/native crypto/HTTP-outbound-aktif → native crate WAJIB, WASI tidak bisa (lihat ADR-001 poin #5, `wasi:http` outbound belum dibuka untuk plugin).

---

## RF-D1 — Orchestrator breakdown (file-split, BUKAN crate-split)

**Kenapa file-split bukan crate-split:** orchestrator secara desain harus tahu SEMUA channel yang aktif (registrasi + dispatch + lifecycle). Memisahkan jadi crate terpisah dari channel-channelnya butuh redesain trait-registry (lihat RF-D3 di bawah sebagai opsi masa depan, bukan sekarang) — risiko tinggi, scope besar. File-split (pola RF-A0 yang sudah proven 4x) jauh lebih murah dan aman.

**Target:** `crates/clawcrew-channels/src/orchestrator/mod.rs` (18.568 baris total, **27 blok `#[cfg(test)]` MASIH CAMPUR inline** meski `orchestrator/tests.rs` sibling sudah ada 33.730 baris — audit 2026-10-03 menemukan RF-A0 untuk orchestrator HANYA memindahkan sebagian test, bukan semua. Ini mengubah urutan kerja RF-D1 jadi 2 sub-langkah wajib berurutan).

**Langkah REVISI (2026-10-03, setelah audit test-coverage):**

### RF-D1a — Tuntaskan test-split SEBELUM body-split
1. Identifikasi 27 blok `#[cfg(test)] mod tests { ... }` (atau variasi nama) yang masih tersisa di `mod.rs` — kemungkinan tersebar di antara fungsi produksi, bukan satu blok besar di akhir (kalau satu blok besar, RF-A0 sebelumnya pasti sudah memindahkannya; 27 blok terpisah mengindikasikan banyak `impl` block punya `mod tests` lokal masing-masing).
2. Untuk tiap blok: pindahkan ke `orchestrator/tests.rs` yang sudah ada (append, dengan `mod` wrapper yang jelas namanya mewakili concern asal — misal `mod registration_tests { ... }`) ATAU, kalau blok itu jelas-jelas milik concern yang akan jadi submodule tersendiri di RF-D1b, tahan dulu dan pindahkan BERSAMAAN dengan body-split-nya supaya tidak dua kali kerja.
3. Verifikasi: hitung total `#[test]`/`#[tokio::test]` sebelum vs sesudah — WAJIB sama.

### RF-D1b — Body-split jadi submodule
4. Baca `mod.rs` penuh (sekali, bukan chunk — pelajaran dari insiden matrix: baca-manual-chunk-kecil bikin subagent stuck 52 menit tanpa progress nyata). Petakan concern yang bercampur: kemungkinan kandidat submodule: `registration.rs` (daftar channel + feature-gate wiring), `dispatch.rs` (routing pesan masuk ke channel yang tepat), `lifecycle.rs` (start/stop/health-check semua channel), `acp_embedded.rs`/`acp_server.rs`/`media_pipeline.rs`/`mqtt.rs` (sudah terpisah sebagai sibling file, verifikasi apakah sudah cukup atau masih ada logic acak di `mod.rs` yang harusnya ikut pindah).
5. Pure-move per submodule, verifikasi `pub(super)`/`pub(crate)` visibility tetap benar (bukan rewrite logic). Test yang ditahan di langkah 2 ikut pindah ke `orchestrator/<submodule>/tests.rs` sesuai concern-nya.
6. **Verifikasi:** `cargo check -p clawcrew-channels` SEKALI di akhir (bukan per-submodule). Jumlah test sebelum/sesudah harus identik — hitung manual via `grep -c '#\[.*test'`.

**DoD:** `orchestrator/mod.rs` tidak lagi single-file >18k baris DAN nol blok test inline; setiap submodule punya tanggung jawab tunggal yang bisa dijelaskan 1 kalimat; `cargo check` hijau; test count sama; tidak ada behavior change (pure file-split).

**Peringatan eksekusi (dari insiden Backend 2026-10-03):** JANGAN baca file 17k+ baris dengan `read` tool chunk kecil berulang (`offset`/`limit` kecil berkali-kali) — itu menghabiskan banyak turn tanpa progress. Baca penuh sekali kalau tool mengizinkan, atau pakai `grep` untuk cari boundary struktural (comment header `// ─── <nama> ───`) dulu sebelum membaca isi lengkap. JANGAN nested-spawn subagent lain untuk "membantu" baca file — body split adalah kerja mekanis linear, bukan paralel.

---

## RF-D2 — Channel crate-split lanjutan (prioritas berdasar 3 prinsip evaluasi + 3 pendekatan bentuk)

**3 pendekatan bentuk — PILIH SALAH SATU per channel/grup, jangan default ke Pendekatan A untuk semua:**

- **Pendekatan A — 1 crate per channel.** Channel independen total (tidak saling panggil fungsi dengan channel lain) DAN cukup besar untuk manfaat compile nyata. Dipakai: irc, amqp, nextcloud_talk, wechat, qq (sudah proven: imessage, signal, bluesky).
- **Pendekatan B — 1 crate gabungan, banyak module di dalamnya.** Beberapa "channel" sebenarnya satu domain yang saling panggil erat (whatsapp_web butuh whatsapp_storage; voice/tts/transcription saling pass data). Crate terpisah per modul di sini MENAMBAH lapisan link tanpa mengurangi coupling nyata. Dipakai: whatsapp+whatsapp_web+whatsapp_storage (1 crate `galleon-channel-whatsapp`), voice-stack (1 crate `galleon-channel-voice`).
- **Pendekatan C — TETAP di monolith, bukan crate Rust sama sekali.** Channel kecil (<~1300 baris), dependency ringan, tidak unik. Kalaupun Tier C (webhook/polling), isolasi yang benar adalah WASI plugin (RF-C2) — isolasi proses, lebih kuat dari crate-split Rust biasa. Memecah dulu jadi crate Rust sebelum porting WASI adalah kerja dua kali untuk nilai yang dibuang. Dipakai: twitch, wecom, clawdtalk, mochat, nostr, reddit, notion, linq, dingtalk, twitter, gmail_push, git.

**Prioritas berdasar 3 prinsip evaluasi (sama seperti sebelumnya):**

Tabel ini SUPERSEDES urutan lama di RF-C1 kalau ada konflik — gunakan evaluasi 3-prinsip di atas sebagai filter final sebelum eksekusi tiap baris, jangan jalankan membuta.

| # | Channel/grup | Baris | Governance-check | Transport-class | Rekomendasi |
|---|---|---|---|---|---|
| D2.1 | `irc` | ~2000 (cek ulang saat eksekusi) | AMAN (nol di util.rs) | persistent TCP → native crate | Crate-split. Pola identik imessage. |
| D2.2 | `amqp` | ~1500 | AMAN | persistent AMQP (lapin) → native crate | Crate-split, batch bareng irc (pola sama) |
| D2.3 | `nextcloud_talk` | 1.494 | AMAN | persistent connection → native crate | Crate-split, batch bareng irc/amqp |
| D2.4 | `whatsapp` + `whatsapp_web` + `whatsapp_storage` | 3.212+7.392+2.234 = 12.838 | **BELUM DIVERIFIKASI** — cek util.rs dulu sebelum mulai | web reverse-eng + session storage native → native crate | Verifikasi governance-check DULU. Kalau aman: 1 crate gabungan `galleon-channel-whatsapp` (3 file jadi 3 module dalam 1 crate, karena saling terkait erat — jangan pecah jadi 3 crate terpisah, itu over-fragmentasi) |
| D2.5 | `wechat` + `qq` | 6.491+2.667 = 9.158 | Pakai `ensure_success`/`read_response_body_limited` (helper teknis, BUKAN governance) → AMAN | protokol kompleks regional CN → native crate | Crate-split terpisah (2 crate, karena wechat dan qq tidak saling terkait meski sama-sama CN). Helper teknis boleh diduplikasi kecil ATAU dipromosikan ke `galleon_channel_core` sebagai util HTTP generik (keputusan Backend saat eksekusi, dengan catatan di PR description) |
| D2.6 | `mattermost` | 5.930 | **BELUM DIVERIFIKASI** — cek util.rs dulu | WS + payload besar → native crate | Verifikasi governance-check dulu sebelum mulai |
| D2.7 | voice-stack (`voice`+`tts`+`transcription`+`voice_call`+`voice_wake`) | 10.154 total | Cek util.rs per-modul dulu | audio pipeline native → native crate | **SATU crate gabungan** `galleon-channel-voice` dengan 5 submodule. JANGAN 5 crate terpisah — mereka saling panggil erat, pecah jadi crate sendiri² menambah lapisan tanpa manfaat compile nyata (over-fragmentasi) |

**Channel yang TIDAK direkomendasikan crate-split (kecil, dependency ringan, biarkan di monolith + file-split RF-A biasa kalau perlu):** `twitch.rs`, `wecom.rs`, `clawdtalk.rs`, `mochat.rs`, `nostr.rs`, `reddit.rs`, `notion.rs`, `linq.rs`, `dingtalk.rs`, `twitter.rs`, `gmail_push.rs`, `git/`. Channel-channel ini masuk radar RF-C2 (WASI plugin) kalau memenuhi kriteria inbound-only — cek dokumen RF-C existing untuk urutan itu, bukan didobel di sini.

---

## RF-D3 — (OPSIONAL, JANGAN EKSEKUSI tanpa approval user terpisah) Orchestrator trait-registry redesign

Kalau setelah RF-D1 (file-split) orchestrator masih dirasa terlalu coupled dengan setiap channel baru yang didaftarkan manual, opsi lanjutan adalah redesain jadi trait-based registry (channel self-register via `inventory`/`linkme` crate, mirip pola yang user tanyakan di awal sesi RF-C). INI ADALAH PERUBAHAN ARSITEKTUR BESAR (one-way-door) — bukan file-split atau crate-split biasa, dan tidak boleh dieksekusi Backend tanpa spec Lead terpisah + sign-off user eksplisit. Dicatat di sini sebagai arah masa depan, bukan task RF-D yang bisa langsung dikerjakan.

---

## Urutan eksekusi yang direkomendasikan (saat Backend availability ada)

1. **RF-D1 dulu** (orchestrator breakdown) — ROI tertinggi, belum tersentuh, blocking value untuk semua channel split berikutnya karena mengurangi conflict surface utama.
2. **RF-D2.1-D2.3** (irc/amqp/nextcloud_talk) — nol-risk, pola sudah proven, bisa paralel dengan RF-D1 kalau ada 2 slot Backend (serialkan tetap WAJIB di crate yang sama, tapi irc/amqp/nextcloud tidak overlap dengan orchestrator).
3. **RF-D2.5** (wechat/qq) — governance-check sudah jelas aman, tinggal eksekusi.
4. **RF-D2.4, RF-D2.6** (whatsapp, mattermost) — WAJIB verifikasi governance-check dulu sebelum mulai, bisa jadi blocker baru seperti matrix.
5. **RF-D2.7** (voice-stack) — terakhir, karena saling terkait erat dan butuh pemahaman penuh 5 modul sekaligus sebelum mulai.

## Verifikasi di setiap langkah (non-negotiable, dari lesson tersimpan)

- STACK semua edit dulu (pure-move mekanis), verifikasi ringan via grep/graphify selama proses, `cargo check` SEKALI di akhir — bukan per-file.
- `target/` normal (hot cache), JANGAN ganti `CARGO_TARGET_DIR`.
- Cek `resource_status` sebelum heavy build kalau memori terlihat tight.
- Hitung test count sebelum/sesudah SETIAP channel/submodule — jangan percaya "terlihat sama", hitung angka.
- Satu writer per crate channel, serialkan — jangan paralel di file/crate yang sama.
