# Rust Clean Architecture — Living Strategy Document

- **Status:** LIVING DOCUMENT — satu sumber kebenaran untuk strategi modularisasi Rust. Update di tempat (edit section ini) ketika ada perubahan kecil; JANGAN buat dokumen RF-baru-per-fase lagi. Dokumen RF-A/B/C/D sebelumnya (`rust-decomposition-plan.md`, `RF-C-*.md`, `RF-D-*.md`) tetap ada sebagai ARSIP HISTORIS — rujuk ke sana hanya untuk detail eksekusi langkah-demi-langkah yang sudah berjalan, bukan untuk keputusan arsitektur baru.
- **Author:** squad-lead
- **Tanggal mulai:** 2026-10-03
- **Terakhir diupdate:** 2026-10-03
- **Prinsip dokumen ini:** satu kategori per komponen, keputusan final dengan alasan konkret (bukan "mungkin bisa"), supaya perubahan kecil di masa depan tinggal update baris tabel yang relevan — tidak perlu nulis dokumen fase baru.

---

## Bagian 1 — Tiga Prinsip Evaluasi (berlaku untuk SEMUA keputusan crate-split di dokumen ini)

Setiap baris di tabel kategorisasi (Bagian 2 dan 3) HARUS lolos 3 cek ini sebelum keputusan "crate sendiri" dijatuhkan. Kalau ada komponen baru di masa depan, jalankan 3 cek ini dan tambahkan 1 baris ke tabel — jangan tulis dokumen baru.

1. **Compile-benefit nyata.** Komponen menarik dependency besar yang TIDAK dipakai komponen lain di crate yang sama (contoh: `matrix-sdk`, `libsignal`, SDK vendor AI tertentu). Komponen kecil dengan dependency ringan dan umum (cuma `reqwest`+`serde`+`tokio` dasar) TIDAK layak crate sendiri — overhead `Cargo.toml`+publish-unit lebih mahal dari manfaatnya.
2. **Governance/state-coupling check.** `grep` file shared-util crate asal (`util.rs`, `dispatch.rs`, dst) untuk nama komponen. Hasil KOSONG atau cuma helper teknis generik (HTTP status check, body-limit, dsb) = aman untuk dipisah. Hasil berupa governance primitive (approval-queue, shared session state, auth token broker) = STOP, komponen itu kemungkinan harus TETAP di crate inti (lihat kasus matrix, Bagian 2).
3. **Transport/execution-class check.** Komponen butuh koneksi persistent/native-crypto/proses-background → crate native Rust. Komponen yang murni request-response tanpa state lintas-panggilan → kandidat isolasi lebih kuat (WASI plugin untuk channel, atau sekadar trait object untuk provider — lihat Bagian 3).

---

## Bagian 2 — Kategorisasi Final: 35 Channel (`clawcrew-channels`, 197.057 baris / 90 file)

### Kategori A — TETAP di `clawcrew-channels` monolith (Tier A produk, governance-coupled)

Channel ini DILARANG dipisah crate kecuali seluruh governance primitive yang mereka pakai (lihat kolom alasan) dipromosikan ke layer core terlebih dahulu — itu sendiri adalah proyek arsitektur terpisah (lihat Bagian 5, Strategi Governance-Core), bukan task crate-split biasa.

| Channel | Baris (mod+tests) | Alasan TETAP |
|---|---|---|
| `telegram` | 7.960+14.095=22.055 | Berbagi `PendingApproval`/`resolve_pending_approval`/`build_yesno_approval_prompt` dengan slack+matrix di `util.rs` |
| `slack` | 9.531+3.927=13.458 | Sama — governance primitive inti |
| `matrix` | 5.129+6.219=11.348 | Sama — ditemukan 2026-10-03 (sebelumnya salah dikategorikan Tier B, lihat histori ADR-001) |
| `discord` | 7.954 (+markers 1.357) | First-party identity produk (primitif transport), bukan governance-coupled tapi tetap Tier A by design |

**Catatan permanen:** kalau ada channel BARU di masa depan yang pakai `PendingApproval`/approval-queue existing, otomatis masuk kategori ini — jangan dipisah crate sampai Bagian 5 selesai.

### Kategori B1 — Crate sendiri, 1-channel-1-crate (independen total + berat)

| Channel | Baris | Dependency unik berat | Status |
|---|---|---|---|
| `signal` | ~2000 | libsignal-ish JSON-RPC/SSE | ✅ DONE (`galleon-channel-signal`) |
| `bluesky` | ~2000 | AT Protocol HTTP client | ✅ DONE (`galleon-channel-bluesky`, deviasi Tier B diterima meski ADR awal bilang Tier C) |
| `imessage` | 1.206 | rusqlite+AppleScript bridge, macOS-only | ✅ DONE (`galleon-channel-imessage`, 2026-10-03) |
| `irc` | 1.209 | persistent TCP, tls-client | Belum — nol governance-coupling, pola identik imessage |
| `amqp` | 1.193 | lapin (persistent AMQP) | Belum — nol governance-coupling |
| `nextcloud_talk` | 1.494 | persistent connection | Belum — nol governance-coupling |
| `wechat` | 6.491 | protokol kompleks regional CN | Belum — pakai `ensure_success` (helper teknis, aman) |
| `qq` | 2.667 | protokol kompleks regional CN | Belum — pakai `ensure_success`+`read_response_body_limited` (helper teknis, aman) |
| `mattermost` | 5.930+ | WS + payload besar | Belum — pakai `ensure_success`-kelas helper (perlu 1x cek ulang sebelum eksekusi, tapi predicted aman) |

### Kategori B2 — Crate gabungan, multi-module dalam 1 crate (saling terkait erat)

Memisah jadi crate per-file di sini MENAMBAH lapisan link tanpa mengurangi coupling — anti-pattern over-fragmentasi.

| Grup | Komponen | Baris total | Nama crate target |
|---|---|---|---|
| WhatsApp | `whatsapp.rs` + `whatsapp_web.rs` + `whatsapp_storage.rs` | 3.212+7.392+2.234=12.838 | `galleon-channel-whatsapp` (3 module dalam 1 crate) |
| Voice-stack | `voice.rs`+`tts.rs`+`transcription.rs`+`voice_call.rs`+`voice_wake.rs` | ~10.154 | `galleon-channel-voice` (5 module dalam 1 crate) |

**Catatan WhatsApp (diklarifikasi 2026-10-03):** `whatsapp`/`whatsapp-web` memang muncul di `util.rs` (shared `voice_reply_skip_reason`, `WhatsAppLocation`) — TAPI ini helper TTS-heuristik dan location-parser antar-2-backend-WhatsApp-sendiri, BUKAN governance primitive seperti matrix. Aman untuk Kategori B2, bukan Kategori A.

### Kategori C — TETAP di monolith ATAU jadi WASI plugin (channel kecil, jangan jadi crate Rust native)

Channel-channel ini kecil (<2000 baris), dependency ringan, dan sebagian besar cocok Tier C (inbound webhook/polling). Keputusan final per channel ada di ADR-001 (Tier C mapping) — jangan buat crate Rust untuk mereka dulu kalau tujuan akhirnya WASI plugin, itu kerja dua kali.

`twitch`, `wecom`, `clawdtalk`, `mochat`, `nostr`, `reddit`, `notion`, `linq`, `dingtalk`, `twitter`, `gmail_push`, `git`, `line`, `lark`.

### Kategori D — Hub/routing, file-split di tempat (BUKAN crate-split sama sekali)

| Komponen | Baris | Kenapa bukan crate-split |
|---|---|---|
| `orchestrator/mod.rs` + `tests.rs` + `acp_server.rs` | 18.568+33.730+10.426=62.724 (31.8% dari total crate) | Hub registrasi+dispatch SEMUA channel — harus tahu semua channel yang ada, tidak bisa dipisah crate tanpa redesain trait-registry (lihat Bagian 5). File-split jadi submodule (`registration.rs`/`dispatch.rs`/`lifecycle.rs`) di tempat. **Status: 27 blok test masih campur inline, belum dieksekusi.** |

---

## Bagian 3 — API Provider / AI Router (`clawcrew-providers`, 80.998 baris / 47 file) — BARU DIANALISIS 2026-10-03

**Jawaban langsung ke pertanyaan "apa provider perlu dipisah crate juga":** YA untuk sebagian, dengan pola EVALUASI SAMA seperti channel (3 prinsip Bagian 1) — bukan treatment berbeda. Provider punya struktur paralel dengan channel: hub routing terpusat (`router.rs`+`dispatch.rs`+`factory.rs`, 6.757 baris gabungan) + implementasi per-vendor yang sudah file-separated tapi satu crate.

**Temuan kunci:** `factory.rs` membangun provider lewat **match string literal manual** (`if family == "openai"`, dst) — pola monolith-match identik dengan channel SEBELUM RF-B1. Ini godfile-pattern yang sama, cuma domain beda.

### Kategorisasi provider

| Provider | Baris | Dependency unik | Kategori | Alasan |
|---|---|---|---|---|
| `openai.rs` | 2.751 | OpenAI SDK wire format | **B1** (crate sendiri) | Independen, dependency wire-format spesifik, cukup besar |
| `openai_codex.rs` | 2.594 | Codex-specific session handling | **B1** | Independen dari openai.rs biasa (beda auth flow) |
| `gemini.rs` | 2.570 | Google Gemini wire format | **B1** | Independen |
| `bedrock.rs` | 2.606 | AWS SDK (berat, unik) | **B1** | Dependency AWS SDK TIDAK dipakai provider lain — kandidat terkuat untuk crate split |
| `openrouter.rs` + `openrouter_catalog.rs` | 2.295+? | HTTP generic, model catalog | **B2** (gabungan, saling terkait) | Catalog adalah data pendukung openrouter, bukan independen |
| `anthropic/` (mod+tests) | 3.160+6.084 | Native provider — **CEK governance-coupling dulu** | **CEK DULU** | `clawcrew-providers/src/anthropic/` perlu di-grep untuk shared state dengan `dispatch.rs` sebelum diputuskan — ini provider PALING SERING dipakai (default), kemungkinan tightly-coupled ke router seperti matrix tightly-coupled ke util.rs |
| `grok_cli.rs` + `grok_cli/acp.rs` | 1.954+1.080 | CLI subprocess bridge | **B2** (gabungan) | acp.rs adalah sub-concern grok_cli, bukan independen |
| `gemini_cli.rs` | 1.954 (cek ulang) | CLI subprocess bridge | **B1** atau **C** tergantung ukuran final | Mirip grok_cli tapi provider beda |
| `azure_openai.rs` | 1.419 | Azure-specific auth (beda dari openai.rs biasa) | **B1** | Independen, auth flow beda dari openai vanilla |
| `copilot.rs` | 1.327 | GitHub Copilot auth flow | **B1** | Independen |
| `ollama.rs` + `ollama_wire.rs` | 1.705+? | Local inference wire format | **B2** (gabungan) | wire.rs adalah sub-concern ollama |
| `hailo_ollama.rs` | 1.561 | Varian ollama untuk hardware Hailo | **C** (tetap monolith atau gabung ke ollama) | Terlalu niche untuk crate sendiri, pertimbangkan merge ke grup ollama sebagai submodule |
| `telnyx.rs` | 13.254 bytes (~400 baris est.) | Voice/SMS provider, kecil | **C** | Kecil, dependency ringan |
| `glm.rs` | 11.676 bytes | Provider kecil | **C** | Kecil |

**Hub yang TETAP (Kategori D, pola sama orchestrator):**

| Komponen | Baris | Kenapa bukan crate-split |
|---|---|---|
| `lib.rs` | 5.840 | Entry point crate, re-export semua provider |
| `router.rs` | 1.638 | Routing logic lintas-provider (model→route resolution) |
| `dispatch.rs` + `dispatch/accounting.rs` | 1.794+877=2.671 | Dispatch + cost accounting, harus tahu semua provider aktif |
| `factory.rs` | 3.325 | Match-string-literal construction — **kandidat refactor ke registry pattern, lihat Bagian 5** |
| `multimodal.rs` | 3.478 | Cross-provider multimodal handling (vision/audio input normalization) |
| `auth/` (mod+profiles) | 2.010+813=2.823 | Auth token broker lintas-provider — **governance-coupled, JANGAN dipisah tanpa cek dulu seperti matrix** |

**Prasyarat sebelum eksekusi provider split (WAJIB, baru ditemukan, belum dikerjakan):**
1. Jalankan 3-prinsip evaluasi (Bagian 1) untuk `anthropic/` — provider default/paling dipakai, risiko tightly-coupled ke `dispatch.rs`/`router.rs` tinggi.
2. Grep `auth/` untuk referensi dari provider individual — kalau `openai.rs`/`gemini.rs`/dst semua memanggil fungsi auth yang sama secara langsung (bukan lewat trait), itu governance-coupling yang harus diselesaikan dulu (promosi ke `clawcrew-providers-core` setara `galleon-channel-core`).
3. **Belum ada crate `clawcrew-providers-core` yang setara `galleon-channel-core`.** Ini harus dibuat SEBELUM provider manapun di-split, supaya shared auth/trait tidak terjebak circular-dependency seperti kasus matrix.

**Status provider split: BELUM DIMULAI SAMA SEKALI.** Ini temuan baru dari sesi ini, belum ada kerja Backend/Lead di area ini.

---

## Bagian 4 — Prinsip Memilih Bentuk Modularisasi (ringkasan keputusan, berlaku channel DAN provider)

| Situasi | Bentuk | Contoh |
|---|---|---|
| Komponen independen total + dependency berat unik | **Crate sendiri (1:1)** | signal, bluesky, imessage, bedrock, azure_openai |
| Beberapa komponen saling panggil erat, dependency sama domain | **Crate gabungan, multi-module** | whatsapp trio, voice-stack, grok_cli+acp, ollama+wire |
| Komponen kecil, dependency ringan, atau akan di-isolasi proses (WASI) | **Tetap monolith / WASI plugin, BUKAN crate Rust** | line, lark, dingtalk, hailo_ollama, telnyx, glm |
| Hub routing yang harus tahu semua anggota domainnya | **File-split di tempat, TETAP SATU CRATE** | orchestrator, router.rs+dispatch.rs+factory.rs |
| Governance/state primitive dipakai lintas-komponen | **TETAP di crate inti sampai dipromosikan ke -core terpisah** | matrix+slack+telegram approval-queue; anthropic+auth/ (dicurigai, belum diverifikasi) |

**Anti-pattern yang harus dihindari (dari pengalaman sesi ini):**
- Memecah SEMUA komponen jadi crate seragam tanpa klasifikasi dulu (over-fragmentasi, biaya `Cargo.toml`+publish-unit tanpa manfaat compile).
- Memecah komponen yang governance-coupled tanpa menyelesaikan dulu shared-state-nya (bikin circular dependency atau duplikasi SSOT — kasus matrix).
- Memecah komponen kecil yang sebenarnya akan di-WASI-kan (kerja dua kali).
- Memecah grup yang saling terkait erat jadi N crate kecil (menambah lapisan link, bukan mengurangi compile unit nyata).

---

## Bagian 5 — Strategi Jangka Panjang untuk Production-Ready yang Robust (beberapa tahun ke depan)

Ini bagian yang menjawab "strategi apa lagi yang masih perlu dirampungkan" — bukan daftar task eksekusi langsung, tapi arah arsitektur yang perlu diputuskan sebelum ditulis jadi task konkret.

### 5.1 — Governance-Core: ekstraksi shared primitive (channel DAN provider)

Baik channel (approval-queue matrix/slack/telegram) maupun provider (auth broker, dicurigai) punya primitive yang dipakai lintas-komponen tapi hidup di tempat yang salah (monolith crate, bukan crate inti yang bisa diimpor dua arah). Opsi jangka panjang:
- Buat `clawcrew-channel-governance-core` (approval-queue, pairing, PendingApproval) sebagai crate yang DIIMPOR OLEH monolith `clawcrew-channels` DAN oleh crate channel individual (matrix/slack/telegram tetap di monolith untuk sekarang, tapi primitive-nya pindah ke crate netral supaya suatu hari channel itu BISA dipisah tanpa circular-dependency).
- Analog untuk provider: `clawcrew-providers-core` untuk auth broker + trait `ModelProvider` dasar.
- **Ini bukan task RF biasa — one-way-door, butuh spec terpisah + sign-off eksplisit sebelum eksekusi.**

### 5.1b — Resilience Layer: fallback-chain + circuit-breaker per-provider (BARU, prioritas tinggi — alasan produk ini ada)

Dikonfirmasi user (2026-10-03): galleon-fleet dibangun SPESIFIK untuk menghindari single-point-of-failure yang dialami di KiroCrew (blocked total saat kredit satu harness habis). Ini menaikkan prioritas robustness provider-layer dari "nice to have" jadi **fitur inti yang harus matang untuk production-ready**.

**TEMUAN KRITIS (audit lintas-bahasa, 2026-10-03): sebagian fitur ini SUDAH ADA — tapi di Go, bukan Rust, dan terpisah dari `clawcrew-providers`.**

`engine/src/llm/` (Go) berisi `provider.go` + `multi_provider.go` + `dispatcher.go`:
- `provider.go`: implementasi OpenAI streaming LENGKAP sendiri — parsing `openAIStreamResponse`, HTTP client langsung ke `https://api.openai.com`, resolusi API key (config → env → Rust vault via gRPC). **Ini bukan proxy ke Rust — ini re-implementasi wire-format OpenAI DI GO**, duplikat domain dari `clawcrew-providers/src/openai.rs` (2.751 baris Rust) yang MELAKUKAN HAL SAMA.
- `multi_provider.go`: `MultiProvider` dengan retry+exponential-backoff+**fallback-chain** PENUH (`StreamWithRetry` — 3x retry per provider, lalu switch ke `fallback` provider). **Ini PERSIS fitur resilience yang didaftarkan sebagai "belum diverifikasi, prioritas tinggi" sebelum temuan ini** — ternyata ADA, tapi di Go.
- Hanya 1 provider real terdaftar (`openai`) + 1 `mock` — BUKAN multi-vendor native (tidak ada `anthropic.go`/`gemini.go`/`bedrock.go` di `engine/src/llm/`). Jadi fallback-chain Go ini saat ini cuma openai→mock, bukan openai→gemini→bedrock yang sebenarnya diinginkan.

**Kesimpulan — ini bukan "Rust belum punya fitur X", ini DUPLIKASI SSOT LINTAS BAHASA yang lebih serius dari kasus matrix:**

`clawcrew-providers` (Rust) punya `router.rs`+`dispatch.rs`+`factory.rs`+multi-vendor penuh (openai/gemini/bedrock/dst, masing-masing lengkap). `engine/src/llm/` (Go) punya `dispatcher.go`+`multi_provider.go`+1-vendor (openai saja) dengan fallback-chain-nya SENDIRI. **Dua sistem LLM-calling independen, bahasa berbeda, kemungkinan dipanggil dari jalur berbeda** (perlu diverifikasi: apakah ada jalur eksekusi yang pakai Go `llm/` dan jalur lain yang pakai Rust `clawcrew-providers` — kalau ya, ini pecah SSOT nyata, bukan cuma dua implementasi tidur).

**Ini melanggar `AGENTS.md` "Single Source Of Truth" dan `galleon-architecture.md` secara langsung** — persis pola yang sudah dicatat sebagai pelanggaran nyata untuk `chat_quartermaster`/risk-tier (Tauri Rust vs Go), kasus LLM-provider ini levelnya LEBIH BESAR (bukan policy kecil, ini seluruh jalur inference).

**Pertanyaan arsitektur yang HARUS dijawab sebelum eksekusi apapun (bukan keputusan Lead sepihak):**
1. Jalur mana yang SEBENARNYA dipakai produksi — Go `engine/src/llm/` atau Rust `clawcrew-providers`? Atau keduanya aktif untuk skenario berbeda (misal Go untuk fleet-orchestration-level call, Rust untuk sesi langsung)?
2. Kalau cuma satu yang seharusnya hidup: SSOT harus di Rust (sesuai alasan produk — kontrol native penuh atas wire-format tiap vendor, yang justru alasan `clawcrew-providers` punya banyak file provider) atau di Go (sesuai posisi Go sebagai "AI Orchestrator" di `galleon-architecture.md`)? Dokumen arsitektur bilang Go = "agent brain, fleet governance" — tapi LLM wire-calling langsung ke vendor API terasa lebih cocok di Rust (native tool execution authority, "System Core" layer).
3. Kalau keduanya memang perlu hidup untuk alasan berbeda, BATASNYA harus didokumentasikan eksplisit (Go panggil Rust lewat gRPC untuk LLM call, bukan Go punya HTTP client openai sendiri) — ini perubahan arsitektur, bukan refactor kecil.

**Status: AUDIT, bukan task eksekusi.** Ini temuan yang butuh keputusan arsitektur user/Lead sebelum ditulis jadi task RF. Mengeksekusi crate-split provider Rust (Bagian 3) TANPA menjawab ini dulu berisiko memperkuat SATU dari dua implementasi yang mungkin salah.

- **Circuit-breaker per-vendor**: Go sudah punya pola retry+backoff (lihat di atas) — kalau SSOT diputuskan di Rust, pola ini perlu diport, bukan dibangun dari nol.
- **Cost-accounting lintas-vendor terpusat**: `dispatch/accounting.rs` (Rust, 877 baris) DAN `engine/src/treasury/` (Go, belum diaudit) kemungkinan juga tumpang-tindih — cek sebelum lanjut.
- **Health-check proaktif per-vendor**: belum ditemukan di kedua bahasa — benar-benar belum ada, bukan duplikasi.

### 5.1c — Full-Autonomous Agent "Perusahaan": implikasi arsitektur (BARU)

Tujuan user: agent yang bekerja full-autonomous setingkat operasional perusahaan ("ships", level produksi). Ini punya implikasi arsitektur yang lebih luas dari sekadar provider resilience:

- **Observability/audit trail tingkat produksi**: setiap keputusan agent (termasuk kapan provider fallback terjadi) perlu tercatat dengan detail yang cukup untuk audit — bukan cuma log biasa. Cek apakah `clawcrew-log` sudah cukup untuk level ini.
- **Rate-limiting & cost-ceiling per-agent/per-task**: agent otonom yang jalan tanpa supervisi manusia butuh pagar biaya yang keras (bukan cuma soft-warning) — kalau tidak, satu task yang salah bisa menghabiskan budget tak terbatas.
- **Graceful degradation, bukan hard-stop**: filosofi "jangan blocked total" dari alasan produk ini ada HARUS diterapkan konsisten di semua layer, bukan cuma provider — termasuk channel (kalau satu channel down, channel lain tetap jalan — ini sudah natural dari arsitektur channel yang terpisah) dan task-execution (task yang stuck di satu approach harus bisa switch strategi, bukan infinite-retry — relevan dengan insiden Backend 764f795c yang stuck 97 menit).
- **Ini bukan task RF Rust biasa** — menyentuh banyak layer (provider, channel, task-runner, observability). Dicatat di sini sebagai arah strategis yang perlu dipecah jadi inisiatif terpisah (bukan RF-E atau nomor baru — ingat prinsip dokumen ini: jangan buat fase baru, breakdown jadi task konkret di section ini kalau sudah siap dieksekusi).

### 5.2 — Self-register sesungguhnya: trait-registry + `inventory`/`linkme`

Pertanyaan awal user di sesi ini ("plugin self-register") paling tepat dijawab di level ini, bukan di level file-split. Orchestrator (`channel`) dan factory (`provider`) sama-sama pakai match-manual untuk mendaftarkan anggota. Redesain ke trait-registry (channel/provider mendaftar diri via macro attribute, bukan ditambahkan manual ke match-statement) akan:
- Menghapus kebutuhan edit `orchestrator/mod.rs` dan `factory.rs` setiap kali channel/provider baru ditambah (mengurangi godfile growth di sumbernya, bukan cuma memecah yang sudah ada).
- Memungkinkan channel/provider pihak-ketiga didaftarkan tanpa rebuild core (prasyarat untuk marketplace plugin yang disebut `galleon-product-fundamentals.md`).

**Status: dicatat sebagai RF-D3 (opsional) di dokumen arsip. Butuh keputusan user eksplisit sebelum spec ditulis — ini mengubah pola kontribusi channel/provider baru secara mendasar.**

### 5.3 — WASI plugin maturity: dari pilot ke produksi

`channel-nostr-pilot` sudah proven 1x. Jalur produksi penuh butuh (belum dikerjakan): (a) bekukan WIT contract v0 setelah 2-3 pilot lagi berhasil (ADR-001 sudah menjadwalkan ini), (b) signature/trust model untuk plugin pihak-ketiga (ada `clawcrew-plugins/src/signature.rs`, belum terpakai produksi), (c) `wasi:http` outbound untuk channel yang butuh posting aktif (twitter/reddit) — ini blocker teknis nyata, bukan prioritas, baru bisa jalan kalau upstream WASI/wasmtime API-nya sudah mendukung.

### 5.4 — Build-time governance berkelanjutan (bukan sekali ukur lalu lupa)

RF-M (sudah ada di roadmap arsip) mengukur baseline sekali. Untuk robust jangka panjang, perlu jadi CI GATE permanen: `cargo build --timings` dibandingkan baseline di setiap PR, bukan diukur manual sesekali. Soft-warning dulu (>2k LOC baru di satu file = warning, bukan block) supaya tidak menghambat kontribusi wajar.

### 5.5 — Dependency-map yang hidup (bukan snapshot sekali graphify)

RF-B0 (dependency map) di roadmap arsip terhenti karena graph.json stale. Untuk robust jangka panjang: jadwalkan `graphify .` regenerate otomatis post-commit (hook sudah ada opsinya: `graphify hook install`), supaya setiap investigasi governance-coupling (seperti kasus matrix) tidak lagi bergantung pada grep manual satu-persatu — itu masih diperlukan untuk verifikasi presisi, tapi graph yang segar akan mempercepat tahap "mana saja yang perlu dicurigai" sebelum grep detail.

### 5.6 — Versioning & publish strategy untuk crate internal

Saat ini semua `galleon-channel-*`/provider-split memakai `publish = false` (atau `true` tapi tidak pernah benar-benar dipublish ke crates.io). Untuk jangka panjang, perlu keputusan eksplisit: apakah crate-crate ini tetap workspace-internal selamanya, atau sebagian (channel/provider yang stabil) akan dipublish terpisah untuk reuse di luar `galleon-fleet`. Ini keputusan produk, bukan teknis murni — pengaruh ke semver policy per-crate.

---

## Bagian 6 — Perbandingan Arsitektur KiroCrew: Pelajaran untuk Provider/AI-Router (BARU, 2026-10-03)

Analisis `providers.md` dan `acp-client.md` resmi KiroCrew mengungkap pendekatan yang **secara filosofis berbeda** dari `clawcrew-providers`, dan ini jawaban langsung ke "mana yang layak dipertimbangkan untuk crate terpisah vs perbaikan arsitektur flow".

### 6.1 — Temuan paling penting: KiroCrew SENGAJA MENOLAK provider-per-vendor

Dikutip langsung dari `providers.md`: *"**Removed, and not to be re-added:** the Bedrock provider, the standalone provider, their config fields, and the multi-provider dispatch factory. A second `agent.provider` value would route around every harness-parity invariant, which is why the enum stays closed."*

KiroCrew hanya punya **SATU** provider konkret: `AcpProvider`. `agent.provider` di-fix ke `"acp"` (enum tertutup, bukan pilihan). Yang bisa dipilih bebas adalah **`agent.acp_backend`** — id harness (kiro-cli/claude/codex/kas/opencode/pi/goose/deepseek) yang diregister di SATU file otoritas (`agent_sdk/backends.py`) sebagai live frozenset membership (`ACP_BACKENDS_KNOWN`, `ACP_BACKENDS_ACP_RUNTIME`, `ACP_BACKENDS_SESSION_SHARING`, dst — bukan if-else per backend, tapi SET MEMBERSHIP yang di-cek positif).

**Ini beda total dari `clawcrew-providers`:** di galleon-fleet, `openai.rs`/`gemini.rs`/`bedrock.rs`/dst adalah **implementasi penuh terpisah** (masing-masing punya wire-format, auth flow, streaming logic sendiri) yang dipilih via `factory.rs` match-string-literal. KiroCrew tidak punya ini sama sekali — SEMUA vendor AI (OpenAI, Anthropic, Gemini, dst) diakses melalui **satu harness `kiro-cli` yang sudah tahu cara bicara ke semua vendor itu**, dan Kiro Crew (layer orchestration) tidak pernah bicara langsung ke vendor API.

### 6.2 — Kenapa KiroCrew menolak pola itu (alasan tertulis, bukan tebakan)

> *"A second `agent.provider` value would route around every harness-parity invariant"*

Artinya: setiap provider konkret tambahan HARUS mereplikasi SEMUA invariant yang sudah dibangun di `AcpProvider` (lifecycle, approval, context-meter, steer, turn-state, session-sharing, compaction handling, liveness oracle, dst — lihat tabel members ABC di `providers.md`, ~40 member). Menambah provider kedua = menduplikasi puluhan invariant itu, dan risiko dua implementasi diam-diam berbeda perilaku adalah PERSIS pola duplikasi-SSOT yang sudah berkali-kali jadi masalah di `clawcrew-channels` (kasus matrix/slack/telegram approval-queue, tapi di skala provider yang jauh lebih besar).

### 6.3 — Apa ini artinya untuk `clawcrew-providers`: KEPUTUSAN FINAL (dikonfirmasi user, 2026-10-03)

**Konfirmasi user:** galleon-fleet SENGAJA bicara native ke tiap vendor AI (OpenAI/Gemini/Bedrock langsung) — ini bukan kebetulan arsitektur, ini **alasan produk ini dibangun**. Latar belakang: produk ini lahir dari keterbatasan nyata KiroCrew — ketergantungan pada SATU harness (`kiro-cli`) berarti saat kredit/kuota harness itu habis, SELURUH sistem blocked, tidak ada jalur alternatif. Galleon-fleet mengambil pendekatan sebaliknya secara sadar: **multi-provider native sebagai fitur ketahanan (resilience), bukan kerumitan yang perlu disederhanakan.**

**Implikasi arsitektur konkret dari tujuan ini:**
- Provider-per-vendor (`openai.rs`/`gemini.rs`/`bedrock.rs`/dst) BUKAN kandidat untuk disederhanakan jadi "1 provider + N backend-selector" — itu akan MENGHILANGKAN fitur inti produk (kalau semua lewat satu harness, satu titik kegagalan itu kembali).
- `router.rs`/`dispatch.rs`/`factory.rs` (hub routing provider) justru harus DIPERKUAT sebagai **failover/fallback layer** — bukan dipandang sebagai godfile yang perlu dibongkar, tapi sebagai lapisan ketahanan yang nilainya SEBANDING dengan alasan produk ini ada. Fallback-chain yang disebut di `providers.md` KiroCrew (`"same-model and fallback-chain retries"`) justru pola yang BENAR untuk ditiru DI DALAM `clawcrew-providers` — bukan dihindari.
- Tujuan jangka panjang user: **agent yang bekerja full-autonomous setingkat operasional perusahaan** ("ships", level produksi tinggi) — ini mengangkat prioritas robustness provider-layer (retry, circuit-breaker per-vendor, cost-accounting lintas-vendor) jadi SAMA PENTING dengan channel crate-split, bukan pekerjaan sekunder.

**Keputusan final: Opsi 1 dikonfirmasi.** Lanjutkan kategorisasi Bagian 3 apa adanya (B1/B2/C per provider). Opsi 2 (redesain ke pola KiroCrew) DITOLAK PERMANEN — bukan karena belum dievaluasi, tapi karena bertentangan langsung dengan alasan produk ini dibangun. Catat ini sebagai keputusan yang TIDAK PERLU DIEVALUASI ULANG di masa depan kecuali tujuan produk berubah secara fundamental.

### 6.4 — Yang TETAP layak diadopsi dari KiroCrew (pola arsitektur, bukan model provider)

Terlepas dari keputusan 6.3, beberapa pola KiroCrew layak dicontoh di `clawcrew-providers`/`clawcrew-channels` TANPA mengubah model provider-per-vendor:

1. **Capability-as-frozenset-membership, bukan if-else per vendor.** KiroCrew menjawab "apakah backend X mendukung fitur Y" lewat `ACP_BACKENDS_<CAPABILITY>` set membership yang dicek POSITIF (bukan `if backend != "x"`). Ini mencegah fitur baru otomatis granted ke backend yang belum diverifikasi. `clawcrew-providers/factory.rs` yang masih match-string-literal (`if family == "openai"`) adalah kandidat tepat untuk pola ini — ganti ke `PROVIDERS_SUPPORT_STREAMING`, `PROVIDERS_SUPPORT_VISION`, dst sebagai frozenset, bukan menambah cabang if di factory setiap provider baru.
2. **ABC/trait surface yang didokumentasikan sebagai kontrak, bukan dibaca dari kode.** `providers.md` punya tabel ~40 member `LLMProvider` ABC dengan makna kontrak masing-masing (bukan dump otomatis). `clawcrew-providers` punya `traits.rs` (1.221 bytes — kemungkinan sangat minimal dibanding kompleksitas aktual) yang BISA diperkaya dokumentasinya dengan pola serupa, tanpa mengubah struktur crate.
3. **Liveness/stall detection yang declared-degradation per platform.** `acp-client.md` punya tabel eksplisit "Platform evidence matrix" yang mengakui macOS/Windows punya evidence lebih sedikit dari Linux, dan setiap gap didokumentasikan sebagai `platform_limited` bukan ditebak. Kalau `clawcrew-providers` punya stall-detection serupa (perlu dicek, belum diverifikasi di sesi ini), pola "declared degradation" ini layak diadopsi.
4. **Governance primitive hidup di LAYER TERPISAH dari implementasi.** PreToolUse Gate (`hooks.py`) terpisah dari provider manapun — provider cuma MEMANGGIL gate itu. Ini balik mengonfirmasi analisa governance-core (Bagian 5.1) sudah di jalur yang benar: approval-queue channel dan auth-broker provider SEHARUSNYA memang di layer terpisah (crate `-core`), bukan di monolith yang providers/channels harus hati-hati tidak merusak.

---

## Bagian 7 — Audit Seluruh Workspace Rust (869.536 baris, 25 crate) — BARU, 2026-10-03

Audit lintas-crate penuh (sebelumnya hanya `clawcrew-channels`+`clawcrew-providers` yang dianalisis dalam). Urutan baris terbesar:

| Crate | Baris | File | Status analisis |
|---|---|---|---|
| `clawcrew-runtime` | 281.927 | 300 | **BELUM DIANALISIS SAMA SEKALI sampai sesi ini** — lihat 7.1 |
| `clawcrew-channels` | 197.057 | 90 | Dianalisis penuh (Bagian 2) |
| `clawcrew-tools` | 85.443 | 96 | **BELUM DIANALISIS** — lihat 7.2 |
| `clawcrew-config` | 81.590 | 36 | **BELUM DIANALISIS** — SSOT config, risiko tinggi kalau godfile |
| `clawcrew-providers` | 80.998 | 47 | Dianalisis (Bagian 3), TAPI lihat 5.1b — duplikasi dengan Go |
| `clawcrew-gateway` | 40.180 | 46 | Sudah di-split (RF-A0, state.rs/gateway.rs/rate_limit.rs) |
| `clawcrew-memory` | 25.437 | 37 | Belum dianalisis, prioritas rendah (ukuran wajar) |
| `clawcrew-api` | 13.465 | 33 | Trait/type definitions, biasanya aman di crate kecil |
| `clawcrew-plugins` | 13.135 | 22 | WASI plugin host — relevan ke RF-C2, belum diaudit internal |
| Sisanya (<11k baris masing-masing) | — | — | Prioritas rendah, ukuran wajar untuk crate tunggal |

### 7.1 — `clawcrew-runtime` (281.927 baris, 300 file) — CRATE TERBESAR, BELUM TERSENTUH

Ini bukan satu godfile seperti orchestrator — ini **crate dengan 28 subdirektori** (`agent/`, `rpc/`, `sop/`, `tools/`, `security/`, `cron/`, `daemon/`, `skills/`, `subagent/`, dst), masing-masing berpotensi domain terpisah. File terbesar di dalamnya:

| File | Baris | Domain |
|---|---|---|
| `agent/loop_/tests.rs` | 15.858 | Agent execution loop |
| `rpc/dispatch/tests.rs` | 11.318 | RPC dispatch |
| `agent/agent/tests.rs` | 11.210 | Agent core |
| `tools/delegate/tests.rs` | 9.610 | Tool delegation (subagent?) |
| `sop/engine/tests.rs` | 9.255 | SOP (standard operating procedure) engine |
| `agent/turn/mod.rs` | 7.216 | Turn execution — **produksi, bukan test** |
| `rpc/dispatch/mod.rs` | 6.895 | RPC dispatch — **produksi** |
| `daemon/mod.rs` | 5.932 | Daemon lifecycle — **produksi** |
| `sop/engine/mod.rs` | 5.919 | SOP engine — **produksi** |

**Temuan struktural:** rasio test-file ke production-file di sini SANGAT tinggi (5 dari 9 file terbesar adalah `tests.rs`) — pola RF-A0 (test-split) kemungkinan SUDAH diterapkan sebagian di crate ini (struktur foldernya sudah modular: `agent/loop_/mod.rs` + `agent/loop_/tests.rs` terpisah). Ini KABAR BAIK — artinya `clawcrew-runtime` sudah lebih rapi secara STRUKTUR FILE dibanding `clawcrew-channels` sebelum RF-A0.

**Yang BELUM diverifikasi (prioritas audit lanjutan, bukan asumsi):**
1. Apakah `agent/turn/mod.rs` (7.216 baris produksi) punya governance-coupling dengan domain lain seperti kasus matrix — perlu `grep` serupa untuk shared primitive.
2. Apakah 28 subdirektori ini punya BATAS yang jelas atau saling import erat — kalau saling erat, ini kandidat crate-split SATU PER SATU (Pendekatan A/B dari Bagian 4) seperti channel; kalau independen, mungkin sudah cukup rapi sebagai monolith besar dengan file-split internal yang baik.
3. `sop/` (SOP engine, 5.919+9.255 baris) — SOP kemungkinan domain yang CUKUP independen (standard operating procedure/workflow definition) untuk jadi kandidat crate sendiri (Pendekatan A) kalau tidak govern-coupled ke `agent/`.

**Rekomendasi: AUDIT LEBIH DALAM diperlukan sebelum kategorisasi B1/B2/C bisa ditulis untuk crate ini** — 282k baris terlalu besar untuk dikategorikan tanpa membaca struktur internal tiap subdirektori. Ini task tersendiri, prioritas TINGGI karena ukurannya crate terbesar di seluruh workspace.

### 7.2 — `clawcrew-tools` (85.443 baris, 96 file) — belum dianalisis

96 file dengan rata-rata ~890 baris/file — secara struktural TERLIHAT sudah granular (tidak ada godfile tunggal yang mendominasi, berbeda dari channels/providers/runtime). Kemungkinan sudah dalam kondisi baik; perlu 1x cek cepat untuk konfirmasi tidak ada file >5k baris yang tersembunyi, tapi BUKAN prioritas refactor berdasar data sejauh ini.

### 7.3 — Hierarki Rust↔Go: TEMUAN KRITIS BARU — duplikasi LLM-provider lintas-bahasa

**Ini jawaban paling konkret untuk "hierarki yang jelas antara Rust dan Go".** `galleon-architecture.md` sudah mendefinisikan batas: Rust = "System Core" (native tools, security microkernel), Go = "AI Orchestrator" (agent brain, fleet governance), komunikasi via gRPC `SystemGateway`. Audit sesi ini menemukan **pelanggaran batas ini yang jauh lebih besar dari kasus matrix/channel**:

`engine/src/llm/` (Go) berisi implementasi LENGKAP pemanggilan OpenAI (HTTP client langsung ke `api.openai.com`, parsing wire-format, retry+fallback-chain) — **BUKAN delegasi ke Rust lewat `SystemGatewayClient`** seperti yang dilakukan `engine/src/llm/dispatcher.go`'s `ToolDispatcher` (yang BENAR delegasi tool-execution ke Rust gRPC — ini contoh batas Rust/Go yang DIIKUTI dengan benar). Sementara itu `clawcrew-providers` (Rust, 80.998 baris) adalah implementasi LENGKAP TERPISAH untuk OpenAI + banyak vendor lain.

**Dua implementasi LLM-calling, dua bahasa, kemungkinan dua jalur eksekusi berbeda yang tidak terkoordinasi.** Ini PERSIS kategori masalah SSOT yang sudah dicatat di `galleon-architecture.md` untuk `chat_quartermaster`, tapi levelnya jauh lebih besar — bukan satu policy kecil, ini **seluruh jalur inference AI**.

Detail konkret dari pembacaan kode:
- `provider.go`: `openAIStreamResponse` struct parsing manual, `httpClient` langsung ke `https://api.openai.com`, resolusi API key 3-tingkat (config flag → env → Rust vault via `gateway.GetDecryptedSecret`).
- `multi_provider.go`: `MultiProvider` dengan `StreamWithRetry` — 3x retry exponential-backoff per provider, lalu switch ke provider `fallback` yang dikonfigurasi. Hanya 2 provider terdaftar: `openai` (real) + `mock` — BUKAN multi-vendor native (tidak ada file `anthropic.go`/`gemini.go`/`bedrock.go`).
- Metric Prometheus (`metrics.LLMTokenUsage`) sudah terpasang di jalur Go ini — berarti ADA observability yang sudah berjalan di jalur ini, bukan prototype buangan.

**Keputusan yang harus dibuat (bukan Lead sepihak — ini arsitektur fundamental):**
- SSOT LLM-calling di Rust (`clawcrew-providers`) ATAU di Go (`engine/src/llm/`), tidak boleh dua-duanya aktif untuk kasus yang sama.
- Kalau SSOT di Rust (lebih konsisten dengan alasan produk — native wire-format control per-vendor adalah fitur, bukan Go yang orchestrate): Go `engine/src/llm/` harus DIBONGKAR, diganti panggilan gRPC ke Rust (perluasan `SystemGateway` proto, bukan cuma native-tool, juga LLM-inference-request).
- Kalau SSOT di Go (konsisten dengan posisi "AI Orchestrator" di dokumen arsitektur, dan metric Prometheus yang sudah terpasang di sana): `clawcrew-providers` yang berlebihan — Rust cukup jadi provider WRAPPER tipis, logic resilience (fallback/retry) pindah total ke Go.
- **Hierarki yang jelas berarti SATU jawaban untuk pertanyaan ini, didokumentasikan di `galleon-architecture.md`, bukan dua implementasi hidup berdampingan "untuk jaga-jaga".**

**Ini adalah temuan PALING LAYAK untuk didiskusikan lebih dulu, sebelum RF-D/provider-split manapun dieksekusi.** Mengerjakan crate-split Rust provider (Bagian 3) sebelum menjawab pertanyaan ini berisiko memoles implementasi yang mungkin akan dibongkar/dipindah ke Go, atau sebaliknya memperkuat Rust sementara Go yang sebenarnya dipakai produksi.

---

## Riwayat perubahan (edit di sini, jangan buat dokumen fase baru)

- **2026-10-03:** Dokumen dibuat. Konsolidasi dari RF-A/B/C/D (arsip) + audit baru provider (`clawcrew-providers`, belum pernah dianalisis sebelumnya) + klarifikasi WhatsApp (aman, bukan governance-coupled seperti matrix) + temuan `anthropic/`+`auth/` provider yang perlu dicurigai sama seperti matrix sebelum eksekusi.
- **2026-10-03 (update):** Tambah Bagian 6 — perbandingan arsitektur KiroCrew (`providers.md`+`acp-client.md` resmi). Temuan utama: KiroCrew SENGAJA menolak provider-per-vendor ("multi-provider dispatch factory" dihapus permanen), memakai 1 provider (`AcpProvider`) + N backend-harness-selector sebagai gantinya, karena provider kedua akan menduplikasi ~40 invariant kontrak ABC.
- **2026-10-03 (update final):** User mengonfirmasi LATAR BELAKANG PRODUK: galleon-fleet SENGAJA dibangun untuk menghindari single-point-of-failure yang dialami di KiroCrew (blocked total saat kredit harness tunggal habis). Multi-provider native BUKAN kerumitan yang perlu disederhanakan — itu ALASAN PRODUK INI ADA. Keputusan Bagian 6.3 difinalisasi: Opsi 1 (lanjutkan provider-per-vendor) dikonfirmasi, Opsi 2 (redesain ke pola KiroCrew) DITOLAK PERMANEN. Tambah 5.1b (Resilience Layer) dan 5.1c (implikasi full-autonomous agent).
- **2026-10-03 (audit workspace penuh):** Tambah Bagian 7 — audit SELURUH 25 crate workspace (869.536 baris total, sebelumnya hanya channels+providers dianalisis). Temuan paling signifikan: (a) `clawcrew-runtime` (281.927 baris, 300 file, crate TERBESAR) belum pernah diaudit sama sekali — perlu audit lanjutan sebelum kategorisasi; (b) **TEMUAN KRITIS** — `engine/src/llm/` (Go) punya implementasi OpenAI streaming+fallback-chain LENGKAP SENDIRI yang duplikat langsung dengan `clawcrew-providers` (Rust) — dua sistem LLM-calling independen lintas-bahasa, pelanggaran SSOT yang jauh lebih besar dari kasus matrix. Ini jadi pertanyaan arsitektur paling mendesak untuk dijawab SEBELUM provider-split Rust manapun dieksekusi.
