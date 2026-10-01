---
title: Galleon — product fundamentals
inclusion: fileMatch
fileMatchPattern: "docs/**/*.md"
---

# Galleon — dasar produk

Ringkasan `docs/finalize/*` (SSOT phase-4) + `docs/fundamental/*` (dasar). Dipakai saat merancang fitur, menamai entitas, atau memutuskan boundary. **Jika bentrok, `docs/finalize/` menang.**

## Apa produknya
Galleon = **local-first, self-hosted AI organization workspace**. Owner ("Pirate King") membangun Fleet berisi Ship (tim AI spesialis persisten); **Quartermaster** = fleet coordinator / **router dua-lapis** (jawab chat ringan langsung + rutekan quest), ber-authority terbatas — **bukan super-agent** (lihat `docs/finalize/01`). Promise: **"Your Fleet. Your Rules."** — BYOK/BYOM/BYOI (user bawa key/model/infra sendiri, bayar provider langsung).

Produk ini **bukan**: reseller kredit, chatbot generik, no-code workflow builder, atau sistem otonom tanpa batas Owner.

## Model operasi (dua hierarki)
- **Organisasi**: Fleet → Ship → Squad → Crew Member (punya skills, tool scope, memory scope, artifact contract, policy/budget).
- **Kerja**: Workspace → Project → Quest → Map → Voyage → Artifact → Discovery → Treasure.
- **Ship ≠ Workspace/Project.** Ship = rumah operasional persisten untuk satu tim spesialis; satu Ship bisa melayani banyak Project/Workspace selama di dalam Charter-nya.

## Prinsip yang mengikat implementasi
- **Artifact before chat**: kerja bernilai berakhir sebagai Artifact (evidence/deliverable terstruktur bertipe) + Discovery → Treasure (divalidasi Owner), bukan prosa transcript. Setiap klaim material butuh evidence link.
- **Autonomy under command**: AI boleh lanjut kerja bounded; aksi high-impact (write/publish/merge/deploy/delete/pay eksternal, ubah Fleet Code, lampaui budget) **wajib Captain's Approval**. Default read-only.
- **Learning by consent**: feedback jadi memory/rule hanya setelah disetujui, scoped (Fleet/Workspace/Project/Ship/Crew/Run), versioned, reversible. Jangan self-modify otomatis.
- **Collaboration lewat Artifact reference + controlled handoff**, bukan forwarding transcript penuh. Cross-Ship memory sharing **default blocked**. Max 1–2 revision cycle lalu eskalasi; Crew tidak spawn agent rekursif tak terbatas.
- **Specialization over generic autonomy**: diferensiasi inti = *Artifact-Driven Persistent Collaboration*, bukan multi-agent spawning generik.
- **Ownership & portability**: data, memory, artifact, Charter, config milik user & bisa export/import.

## Authority (ringkas — detail di `01`/`07`/`18`)
- Quartermaster: broad awareness + orchestration, **narrow direct execution**. Bisa plan/route/propose/spawn temporary agent/baca summary; **tidak bisa** baca raw secret, aksi eksternal, ubah Fleet Code, lampaui budget tanpa approval Owner.
- **Approval binding**: approval terikat ke action digest (policy version, identity, tool, target, redacted args, credential scope). Perubahan apa pun membatalkan approval.
- Risk tiers: `read_only` → `draft` → `write` → `sensitive` → `destructive` (map ke package `policy`/`tool` Go — **satu sumber**, jangan duplikat di Rust/Go dengan nilai beda; lihat `galleon-architecture.md` SSOT).

## Penamaan — lore vs kode (WAJIB)
- Istilah naratif (Pirate King, Quartermaster, Ship, Quest, Treasure, Harbor, Treasury...) hanya untuk **UX/onboarding/marketing/empty-state**.
- **Kode/API/DB/SDK pakai istilah netral**: `fleet`, `workspace`, `project`, `ship`, `squad`, `crew`, `workflow`, `run`, `artifact`, `policy`, `approval`, `audit_event`, `budget`, `integration`. Jangan kunci istilah lore ke namespace teknis yang mahal diubah.
- `Timber` = metafora kapasitas visual, **bukan** mata uang kredit inference. Jangan jual "timber" konsumabel.

## Bounded context engine (`engine/src/`)
Scaffold baru: `orchestrator, fleet, ship, squad, charter, mission, navigator, briefing, policy, approval, treasury, audit, entitlement, schedule, integration`. **Extend, jangan duplikat** yang sudah ada: `crew, run, workflow, task, tool, llm, memory, artifact, persistence`. Plumb `correlation_id` lintas Quest→Voyage→Artifact→Ship Report.

## Monetisasi (jangan reproduksi credit lock-in)
Charge untuk ekspansi/Charter/maintenance/kolaborasi/support — **bukan** akses dasar ke intelligence milik user. Community = 1 Ship aktif, ≤5 Crew, 2 Voyage, delegation depth 1. Jangan kunci Artifact/memory user di balik paywall.

### INVARIANT F1 — Treasury read-only cost (mengikat implementasi)
Galleon **tidak pernah berdiri di antara user dan tagihan providernya** (BYOK/BYOM). Treasury hanya **melaporkan** biaya; ia bukan dompet, bukan saldo kredit, bukan meteran konsumsi.

Aturan yang mengikat (langgar = PR ditolak):
1. **Treasury lapor, tidak memotong.** Biaya provider ditampilkan dalam **USD apa adanya** (sum `costUSD` per entri ledger: date, quest, provider, model, tokens, costUSD). Dilarang ada saldo yang di-*debit*/di-*deduct*/di-*charge* per-inference/per-token sebagai mata uang internal. Fakta kode saat ini: `web-2/src/components/features/TreasuryView.tsx` = ledger read-only murni (`treasuryLedger.reduce(costUSD)`), budget cap USD + soft-warning/hard-cap display; `engine/src/treasury/interfaces.go` masih stub (`// TODO`) — belum ada jalur yang melanggar; invariant ini menjaga agar tetap begitu saat Treasury diimplementasi.
2. **Zero platform markup.** User bayar provider langsung; tak ada margin/markup Galleon di atas harga provider. UI sudah menegaskan ini (`TreasuryView` "zero platform credit markup", `HarborView` "Zero forced credits", `RightRail` "No credit lock-in") — kode tak boleh kontradiksi.
3. **Budget = pembatas, bukan dompet.** Cap/threshold (soft-warning, hard "drop-anchor" cap) hanya **menghentikan** aksi berbiaya saat terlampaui (lewat Captain's Approval tier `sensitive`/budget-exceed) — bukan mengurangi saldo berbayar. Lampaui budget = butuh approval (lihat "Autonomy under command"), bukan beli kredit.
4. **`Timber` = kapasitas, bukan konsumsi inference.** `Timber` mengukur kapasitas organisasi (jumlah Ship/Berth/Squad Charter/Voyage aktif) dan di-charge sebagai *scaling* — **tidak pernah** berkurang per inference/token. Fakta kode: `web-2/src/components/features/ShipyardView.tsx` = Timber capacity meter, "We charge for organizational scaling—never for model inference credits".

Acceptance (observable): tidak ada field/metode di `engine/src/treasury/**` atau store/komponen `web-2/**` yang (a) menyimpan saldo kredit internal yang berkurang per pemakaian LLM, atau (b) menghitung biaya tagih = cost provider + markup. Treasury hanya membaca cost yang sudah terjadi dan membandingkannya ke cap. Pelanggaran dapat dijadikan gate di `tests/architecture/` (lihat F2).

North-star lihat di bawah; jangan optimalkan konsumsi token sebagai revenue — itu bertentangan dengan "Your Fleet. Your Rules."


## North-star
**Verified Useful Treasures per Active Ship per Week** (Quest selesai tanpa pelanggaran policy material, Artifact diterima Owner/policy, berguna+ber-evidence, dalam budget, tanpa insiden korektif besar).
