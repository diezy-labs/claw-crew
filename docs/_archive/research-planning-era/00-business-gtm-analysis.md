# Galleon Fleet — Business, Positioning & Go-to-Market Analysis

> **Status:** Analisa strategis (2026-10-01). Ditulis sebagai tinjauan jujur untuk solo dev.
> **Konteks:** Fork dari [zeroclaw](https://github.com/zeroclaw-labs/zeroclaw), konsep diadaptasi dari Kiro Crew.
> **Sumber yang ditinjau:** `docs/fundamental/`, `docs/finalize/`, `docs/monet/`, `docs/re-branding/`, dan status implementasi nyata di `docs/finalize/08-next-plan-tasks.md`.
> **Hirarki keputusan:** `docs/finalize/` menang untuk konflik bisnis (phase-4 SSOT).

---

## 1. Apa yang sebenarnya dimiliki (ringkas & jujur)

**Produk:** Galleon = *AI organization workspace self-hosted*. Metafora bajak laut: Pirate King (owner) → Quartermaster (koordinator) → Fleet → Ship → Squad → Crew → Quest → Artifact/Treasure. Arsitektur 3-tier: Tauri shell + Rust core (security/sandbox) + Go engine (orchestrator).

**Pembeda inti** (dari `finalize/07`, dinilai benar):
> Kompetitor memberi **satu asisten AI kuat**. Galleon memberi **organisasi AI** — tim spesialis persisten yang dikoordinasi. *Your key, your model, your server — dan sekarang, your org.*

**Status nyata (penting untuk GTM — jangan over-promise):**
- ✅ Sudah nyata: DiskStore persistence wired (M1), Quartermaster router dua-lapis + chat LLM (M2), learning-by-consent (M3), risk-tier SSOT di Go.
- 🔨 Belum: Rust SystemGateway sandbox exec (F3-2 skip), seed data masih di TS (F4-1/4/5 sebagian), metrics masih konstanta (F5-1), cron no-LLM (F5-2), modelgateway swappable (F5-3).

**Implikasi:** loop "satu Developer Ship end-to-end" belum 100% tembus. Ini fakta GTM paling penting.

---

## 2. Penilaian strategi yang sudah ada

### Yang sudah benar (pertahankan)
- "No credit gate" sebagai narasi anti-pain Kiro Crew → tajam, jujur, emosional. Wedge nyata.
- Monetisasi jual **kapasitas + Charter + support**, bukan token. Benar secara etis & posisi.
- Pembeda "organisasi berlapis vs satu agen" — membedakan secara konsep dari OpenClaw/Hermes/Kiro Crew.

### Yang perlu dikoreksi keras

**a) "Pembeda" organisasi berlapis = juga risiko terbesar.** Fleet→Ship→Squad→Crew→Quest→Voyage→Artifact→Treasure = 8+ konsep baru sebelum user dapat nilai. Berisiko jadi *beban kognitif*, bukan delight — terutama untuk persona target sendiri (developer/power-user teknis yang sering benci gamifikasi penutup fungsi). **Saran: lore opt-in / tipis di awal, fungsi dulu.** Doc sudah bilang "lore = experience layer, functional clarity = business layer" — tegakkan agresif.

**b) Solo-dev vs scope.** Visi penuh (7 fase, 6 jenis Ship, marketplace Charter, Private Dockyard) = kerja tim 2 tahun. Keputusan "satu Developer Ship dulu" (`fundamentals §19`) benar — disiplin eksekusi = pembeda hidup/mati solo dev.

**c) Time-to-first-value = musuh #1.** Onboarding `06-business-and-ux §16.3` punya 13 langkah sebelum Treasure pertama. Produk self-hosted + BYOK + build Rust+Go+Tauri sudah friksi tinggi. Setiap langkah = user hilang.

---

## 3. Analisa pasar & persaingan

| Sumbu | Kiro Crew | OpenClaw (391k★) | Hermes | **Galleon** |
|---|---|---|---|---|
| Deployment | gateway Python | self-host lokal | runtime | **self-host, Rust kecil** |
| Unit mental | 1 asisten/session | 1 asisten + channel | 1 agen+skill | **organisasi berlapis** |
| Lock-in | credit-gated | tidak | tidak | **tidak (BYOK)** |
| Maturity | 4.2k★, hidup | 391k★, besar | MIT, aktif | **fork pre-launch** |

**Realita kompetitif:**
- "Local-first + BYOK + persistent" **bukan pembeda** — meja taruhan (semua punya). Doc sudah akui (`finalize/07 §4`). Benar.
- OpenClaw (391k★ + marketplace + 20+ channel) selalu menang di breadth. Jangan lawan di sana (doc sudah bilang "jangan kejar channel/companion/marketplace sekarang" — benar).
- Tanah yang bisa dimenangi: **"struktur tim + governance + evidence-first untuk kerja berulang yang serius"**, bukan "asisten serba bisa."

**Siapa yang akan bayar?** "Developer yang mau organisasi AI self-hosted dengan governance" = niche dalam niche. Volume kecil, tapi **membayar** jika nyeri nyata (audit, approval, cost control untuk kerja agentic repetitif) terselesaikan. Harap depth-of-value, bukan viral-growth.

---

## 4. Rekomendasi Go-to-Market (solo dev, realistis & anti-hype)

**Fase 0 — Tutup loop satu Ship (0–8 minggu).** Jangan sentuh marketing sampai alur ini jalan end-to-end di mesin orang lain:
> Install → connect provider → "analisa repo & siapkan rilis" → 3 Crew bekerja → Artifact nyata (Repo Health Brief + Changelog + Go/No-Go) → approve → GitHub issue draft.
>
> = Milestone 1–3 + F4-1/F5-1. Tanpa ini, tidak ada GTM.

**Fase 1 — Wedge sempit, bukan platform (minggu 8–16).**
- Satu persona: solo dev / indie / konsultan dengan 1 repo aktif ("Engineering Operations Ship", `monet §4.4`).
- Satu janji terukur: *"AI crew yang menyiapkan rilis & triage PR repo-mu, lokal, dengan API key-mu sendiri, dan kamu approve sebelum apa pun ditulis."*
- **Bukan** "organisasi AI untuk segala kerja."

**Fase 2 — Distribusi (channel realistis solo dev):**
1. Open-source di GitHub, **build in public.** Fork MIT/Apache = leverage. ★ = kredibilitas niche developer.
2. **Konten teknis, bukan iklan.** "Cara auto-triage PR dengan AI crew lokal tanpa kirim kode ke cloud."
3. **Hacker News / r/selfhosted / r/LocalLLaMA / Lobsters.** Audiens self-host+BYOK+privacy ada di sini. Satu Show HN jujur > 100 tweet.
4. **Discord kecil + Charter examples.** Komunitas niche yang dalam.

**Fase 3 — Monetisasi (jangan buru-buru):** open-core sudah benar. Urutan:
- Community gratis (1 Ship, 5 Crew) → harus benar-benar berguna (akuisisi).
- Pro = kapasitas (multi-Ship) → laku **setelah** 1 Ship mentok. Jangan jual kapasitas sebelum ada yang butuh.
- **Charter Packs** = monetisasi paling menjanjikan solo dev: jual blueprint tim siap-pakai (Developer Delivery, Content Studio). Bikin sekali, jual berkali-kali. Margin tinggi, maintenance rendah.
- Services/Private Dockyard = nanti, hanya kalau demand berulang.
- Pricing Indonesia-aware: Mayar.id/QRIS untuk Charter one-time; subscription Pro tahan sampai retensi terbukti.

---

## 5. Risiko bisnis terbesar (urut prioritas)

1. **Over-scope membunuh solo dev.** Mitigasi: kunci 1 Ship sampai ada 10 user aktif mingguan nyata.
2. **Klaim "persistent/autonomous" belum semua nyata** (F3-2 skip, metrics konstanta). Pasarkan hanya yang ✅ — `AGENTS.md` sendiri melarang klaim fiktif.
3. **Metafora bajak laut mengalienasi developer skeptis.** Mitigasi: mode "plain" (Owner/Orchestrator/Workspace/Agent) sebagai default, lore sebagai skin opsional. Kode sudah netral (`crew_member`, `fleet`) → murni keputusan UI copy.
4. **Komoditisasi.** Model foundation makin pintar sendiri. "Governance + evidence-first + team structure" harus jadi nyeri nyata — validasi dengan 5 user asli dulu.
5. **Legal fork** (lihat §6).

---

## 6. Catatan hukum fork (bukan nasihat hukum final — perlu cek sendiri / advokat)

- **Lisensi kode:** repo dual MIT/Apache-2.0 → fork & jual boleh, tapi **wajib pertahankan atribusi/NOTICE/LICENSE.**
- **Trademark:** nama/logo "ClawCrew" = trademark pemilik lama. Rebrand penuh ke **Galleon** benar & perlu. Pastikan aset marketing/README/web **bersih** dari nama/logo/domain lama (`clawcrew-*` di kode internal lama OK; aset publik wajib bersih).
- **Impersonation notice** README lama menargetkan fork tak-resmi — posisikan jelas sebagai fork independen berlisensi, bukan mengklaim afiliasi.
- Konsep (ide "crew/persistent workspace") tak bisa di-copyright; implementasi sendiri aman selama bukan menyalin kode berlisensi ketat.

---

## 7. Rencana 90 hari (konkret, solo dev)

| Minggu | Fokus | Hasil |
|---|---|---|
| 1–6 | Tutup loop 1 Ship (M1–M3 + F4-1, F5-1) | Demo jujur jalan di mesin lain |
| 7–8 | Rebrand aset publik bersih + landing 1 halaman | "Your Fleet. Your Rules." + demo video 2 menit |
| 9–12 | Show HN / r/selfhosted + Discord + 1 Charter Pack contoh | 10 user aktif mingguan, feedback nyeri nyata |
| Setelah itu | Monetisasi HANYA jika retensi terbukti | Charter Pack berbayar pertama |

**North-star (sudah benar):** *Verified Useful Treasures per Active Ship per Week.* Kalau > 0 untuk 10 orang asing → ada bisnis. Kalau tidak → tak ada tagline yang menolong.

---

## Penutup

Strategi di atas kertas sudah A-. Risiko nyata bukan strategi — tapi **eksekusi fokus sebagai solo dev** dan **jarak antara klaim produk vs kode yang benar-benar jalan.** Tutup loop satu Ship, pasarkan hanya yang ✅, menangi niche sempit dulu.
