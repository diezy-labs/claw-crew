# 02 — PRD (Phase 4 Finalize)

> Menyatukan `docs/fundamental` (dasar produk) + `docs/re-branding/03-prd` (PRD teknis) jadi PRD eksekusi akhir.

## 1. Masalah
AI work berhenti saat kredit platform habis; user tak bisa pilih intelligence/infra/aturan sendiri; kerja agen hilang saat sesi/aplikasi tutup; multi-agent generik mahal & gagal diam-diam tanpa governance.

## 2. Pengguna
- **Pirate King (Owner)** — pemilik & otoritas final. Developer/power-user (profil: nyaman penuh teknis).
- **Quartermaster** — asisten AI fleet-coordinator (router dua-lapis, lihat `01`).
- Peran AI internal: Captain (per-Ship), Squad Lead, Crew Member, Temporary Agent.

## 3. Tujuan produk
1. **Local-first, BYOK/BYOM/BYOI** — user bawa key/model/infra, bayar provider langsung. Galleon tak pernah jadi credit-gate.
2. **Persistent** — Ship/Crew/memory/artifact bertahan lintas restart (butuh DiskStore nyata).
3. **Artifact-driven** — kerja bernilai berakhir sebagai Artifact→Discovery→Treasure, bukan prosa chat.
4. **Autonomy under command** — aksi high-impact wajib approval; default read-only.
5. **Quartermaster sebagai asisten + coordinator** — jawab chat ringan DAN rutekan quest (satu pintu).

## 4. User stories inti (phase-4)
- Sebagai PK, saya bertanya hal umum ke Quartermaster & dapat jawaban langsung (tanpa dipaksa bikin quest). → `01` cabang `chat`.
- Sebagai PK, saya minta "siapkan rilis vX" → Quartermaster usul FleetOrderProposal → saya approve → Captain eksekusi → saya terima Treasure. → cabang `objective`.
- Sebagai PK, saya minta ringkasan status semua Ship → Fleet Report. → cabang `report`.
- Sebagai PK, saya cek Engine Room (RAM/health/budget). → cabang `engine_room`.
- Sebagai PK, koreksi saya jadi lesson scoped & reversible hanya setelah saya setujui. → learning-by-consent.

## 5. Scope phase-4 (end-to-end, satu Developer Ship dulu)
IN: Quartermaster router dua-lapis; DiskStore persistence; ChatQuartermaster via LLM; metrics/diagnostics nyata; SSOT data di Go (bukan `seedData.ts`); tool execution lewat sandbox Rust; frontend de-mock + tipe ketat.
OUT (tunda): multi-Fleet, Marketing/Research/Operations Ship penuh, Fleet Knowledge F4, Private Dockyard. (Fundamentals §19 MVP: buktikan satu Developer Ship end-to-end dulu.)

## 6. Success metrics
- **North-star:** Verified Useful Treasures per Active Ship per Week.
- Aktivasi: time-to-first-Artifact; time-to-first-Treasure.
- Kualitas: Artifact acceptance rate; revision rate; evidence completeness.
- Trust: approval rate; policy-denied rate; emergency-stop usage.
- Biaya: cost per Treasure (dari provider BYOK, transparan read-only).
- Router health: distribusi intent; fallback-intent frequency; mis-route (dari koreksi user).

## 7. Non-goals (invariant branding)
- BUKAN credit reseller; `Timber` = metafora kapasitas, bukan unit inference.
- Quartermaster BUKAN super-agent (lihat `01`).
- Tidak mengunci Artifact/memory user di balik paywall.
