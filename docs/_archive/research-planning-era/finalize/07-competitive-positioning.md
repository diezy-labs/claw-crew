# 07 — Competitive Positioning & Tech-Gap (Phase 4 Finalize)

> Gabungan analisa kompetitor direct/horizontal/cross. Faktual dari repo/docs resmi (2026-10). Dijaga ringkas (hemat token = bagian dari visi).

## 1. Peta kompetitor

| Produk | Kekuatan inti | Arsitektur |
|---|---|---|
| **Kiro Crew** (4.2k★) | Persistent session, task-spec checkpoint/resume, cron no-LLM, self-learning lessons, subagent fan-out | Gateway → agent sessions (Python, ACP) |
| **OpenClaw** (391k★) | 20+ channel, companion apps (voice/camera/screen), plugin SDK + ClawHub, pairing untrusted sender | Gateway lokal (TS/Rust): "trusted gateway, untrusted execution, deterministic policy" |
| **Hermes** (Nous, MIT) | Learning loop sejati (skill dari pengalaman, perbaiki saat pakai, self-nudge persist, cari chat lampau), 70+ tool, 200+ model | Runtime wrap LLM + tool-loop + memory + skill |

## 2. Tech-gap Galleon

| Gap | Punya | Prioritas |
|---|---|---|
| Persistence nyata (state lintas restart) | KC/OpenClaw/Hermes | 🔴 KRITIS — DiskStore belum di-wire; "persistent" masih fiktif |
| Task checkpoint/resume | Kiro Crew | 🔴 Voyage state machine harus adopsi |
| Learning loop (consent-gated) | Hermes (auto) / KC (lessons) | 🟠 mekanisme MemoryProposal belum ada |
| Cron no-LLM / scheduled | Kiro Crew | 🟠 hemat token — inti visi |
| Model/harness swappable (BYOM 200+) | OpenClaw/Hermes | 🟠 modelgateway perlu diperkuat |
| Channel breadth / companion apps | OpenClaw | 🟡 tunda |
| Plugin/Charter marketplace | OpenClaw (ClawHub) | 🟡 tunda |

## 3. Yang diadopsi (prioritas)

1. **Task-spec checkpoint/resume** (dari KC) → pola Voyage: plan→exec→validate→retry→resume.
2. **Cron no-LLM** (dari KC) → scheduled polling/monitor TANPA LLM call. Langsung dukung visi anti-boros-token.
3. **Self-improving skill loop** (dari Hermes) → versi Galleon **consent-gated** (MemoryProposal approval), bukan otomatis.
4. **Trusted-gateway/untrusted-execution/deterministic-policy** (dari OpenClaw) → sudah ada di tier Rust; pertahankan + dokumentasikan sbg pembeda keamanan.
5. **Model-as-swappable-plugin** (OpenClaw/Hermes) → perkuat `modelgateway` agar BYOM plug-and-play.

**JANGAN kejar sekarang** (boros, bukan diferensiator): 20+ channel, companion voice/camera, marketplace penuh. Tunda sampai loop inti terbukti (fundamentals §19: satu Developer Ship end-to-end dulu).

## 4. Positioning & key differentiator

Local-first + BYOK + persistent + skills = **meja taruhan** (semua kompetitor punya), BUKAN pembeda. Pembeda sejati Galleon:

| Dimensi | KC / OpenClaw / Hermes | **Galleon** |
|---|---|---|
| Unit mental | 1 asisten / session flat | **AI organization berlapis** (Fleet→Ship→Squad→Crew) |
| Spesialis | subagent ephemeral / skill | **Crew Member persisten** (role+skill+model+budget contract, punya rumah) |
| Koordinasi | agen pilih tool sendiri | **Quartermaster router eksplisit** + Captain orchestrator (dua lapis, auditable) |
| Learning | otomatis (Hermes) | **by-consent, scoped, reversible** |
| Hasil | chat/response | **Artifact → Discovery → Treasure** (evidence-first) |
| Governance | policy (OpenClaw) | **risk-tier 5-level + approval binding + Fleet Code** |

**One-liner:**
> KC/OpenClaw/Hermes memberi **satu asisten AI yang kuat**. Galleon memberi **organisasi AI** — armada tim spesialis persisten yang dikoordinasi, bukan satu agen serba bisa. *Your key, your model, your server — dan sekarang, your org.*

## 5. Selaras visi "AI local 1st, no credit/token/cloud down"

- Semua pembeda = lapisan organisasi di atas intelligence milik user; **nol ketergantungan cloud/kredit**.
- Cron no-LLM + router rule-first + Crew persisten (bukan re-spawn) = **hemat token by design** — menguatkan visi.
- `Timber` = kapasitas, bukan kredit inference. BYOK cost transparan read-only.
