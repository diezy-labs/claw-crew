# 06 — Core Business, Persona & Frasa (Phase 4 Finalize)

> Menyelaraskan landasan bisnis + bahasa produk `docs/fundamental` (01/02/15/21) dengan hasil analisa kompetitor phase-4. **Satu koreksi penting:** Quartermaster diposisikan sebagai **fleet coordinator** (router dua-lapis), BUKAN "super-agent" — frasa marketing menyesuaikan.

## 1. Landasan bisnis

### Thesis
> AI work tidak boleh berhenti saat kredit platform habis. User memilih intelligence, infrastruktur, struktur tim, dan aturan operasi sendiri.

### Apa yang user MILIKI vs produk SEDIAKAN
| User owns | Galleon provides |
|---|---|
| Akun/API provider, pilihan model, endpoint lokal, infra, data, memory, artifact | Orkestrasi Fleet, struktur Ship/Squad/Crew, Mission Board, Artifact/Treasury/Logbook, policy/approval/learning, safety UX, kolaborasi, kapasitas |

### Monetisasi (jangan reproduksi credit lock-in)
Charge untuk **ekspansi kapasitas, Charter siap-pakai, maintenance, kolaborasi, support, deployment** — BUKAN akses dasar ke intelligence milik user.
- **Community (gratis):** 1 Fleet, 1 Ship aktif, 1 Quartermaster, ≤5 Crew, 2 Voyage, delegation depth 1, BYOK/BYOM/BYOI, read-first + approval untuk write.
- **Pro / Charter packs / Fleet Care / Team / Private Dockyard / Services** = lapisan berbayar.
- **Timber = metafora kapasitas visual** (jumlah Ship/Crew/Voyage), **bukan** mata uang inference. Jangan jual "timber" konsumabel.

### North-star
**Verified Useful Treasures per Active Ship per Week** — Quest selesai tanpa pelanggaran policy material, Artifact diterima Owner/policy, berguna+ber-evidence, dalam budget, tanpa insiden korektif.

### Diferensiasi inti
**Artifact-Driven Persistent Collaboration** — spesialis AI persisten berkolaborasi lewat Artifact + handoff terkontrol + quality gate + memory scoped + learning disetujui + aksi ber-approval. Bukan swarm multi-agent generik.

## 2. Persona (vocabulary — lore vs kode)

| Narasi (UX/marketing) | Fungsional (layar safety/cost) | Kode/API/DB (netral) |
|---|---|---|
| Pirate King | Owner | `owner` / `pirate_king` |
| **Quartermaster** (Quarterclaw) | Fleet Coordinator & Governance Assistant | `quartermaster` |
| Fleet / Dermaga | Organization | `fleet` |
| Ship | Persistent AI Team | `ship` |
| Captain | Ship Orchestrator | `crew` (StartTurn) |
| Squad | Functional Team | `squad` |
| Crew Member | AI Specialist | `crew_member` |
| Quest / Map / Voyage | Workflow / Plan / Run | `workflow`/`map` · `run`/`voyage` |
| Artifact / Discovery / Treasure | Deliverable / Finding / Validated outcome | `artifact` |
| Harbor / Treasury / Logbook / Crow's Nest | Integrations / Budget / Audit / Monitoring | `integration`/`budget`/`audit_event` |
| Timber | Capacity metaphor | (bukan field kredit) |

**Aturan penamaan (WAJIB):** istilah naratif hanya di UX/onboarding/marketing/empty-state; kode/API/DB/SDK pakai istilah netral; jangan kunci lore ke namespace teknis.

## 3. Persona Quartermaster (identitas & suara)

- **Peran resmi:** *Quartermaster — Fleet Coordination & Governance Assistant.* Codename UI opsional: **Quarterclaw**.
- **Misi:** "Bantu Pirate King mengomando banyak Ship tanpa menjadi otoritas komando yang tak terkendali."
- **Bukan:** super-agent, admin tak terbatas, eksekutor otonom. (Koreksi atas framing lama.)
- **Suara:** ringkas, hormat, decision-oriented. Seperti chief-of-staff: menyodorkan opsi + rekomendasi + biaya/risiko, bukan basa-basi. Jujur soal yang belum diverifikasi.
- **Dua wajah (satu pintu, lihat `01`):**
  - *Asisten pribadi* — jawab pertanyaan umum langsung, hangat & cepat.
  - *Executive coordinator* — untuk objective: analisa → usul FleetOrderProposal → tunggu komando.

## 4. Frasa produk (copy kanonikal)

### Promise & hero
```
Your Fleet. Your Rules.
Bangun organisasi AI self-hosted dengan Quartermaster, Crew spesialis,
dan Ship persisten yang bekerja pada tujuan Anda.
Pilih provider, model, dan infrastruktur Anda sendiri.
Autonomous work, under your command.
```

### Deskripsi Quartermaster (UI singkat)
```
Koordinator fleet Anda. Quartermaster menata prioritas, meringkas laporan Ship,
memantau sumber daya & risiko, menjawab pertanyaan Anda, dan mengeskalasi
hanya keputusan yang butuh komando Anda.
```

### Framing yang DIPAKAI vs DIHINDARI
| Pakai | Hindari |
|---|---|
| "Fleet coordinator", "under your command", "propose & escalate" | "Super-agent", "fully autonomous CEO", "does everything" |
| "You pay your provider directly", "BYOK/BYOM/BYOI" | "AI credits", "token wallet", vague "BYOK" sendirian |
| "Autonomous work, under your command", "Independent, not ungoverned" | "Unlimited autonomy", "set and forget" |
| "Timber = kapasitas Fleet" | "beli timber", "timber habis" |

### Clarity statement (wajib di pricing)
```
Penggunaan model AI tidak termasuk. Anda menghubungkan & membayar provider pilihan Anda langsung.
Paket produk membuka kapasitas Fleet, tim spesialis, Charter, kolaborasi, dan support — bukan token AI.
```

## 5. Catatan selaras (apa yang berubah dari fundamental)
- Fundamental menyebut Quartermaster "AI CEO / super agent"; analisa phase-4 **mempertajam**: tetap "AI CEO" sebagai *metafora peran koordinasi*, tapi secara teknis & frasa = coordinator ber-authority terbatas (router), bukan eksekutor. Ini menghindari over-promise dan selaras permission profile `re-branding/07`.
- Semua pilar lain (BYOK, persistent, artifact-first, learning-by-consent, monetisasi, north-star) **tidak berubah** — dikunci di sini sebagai SSOT bisnis phase-4.
