# 03 — Design & UX (Phase 4 Finalize)

> Dasar: `docs/fundamental/design/*` + `docs/re-branding/15-ui-ux-design`. UX = lapisan pengalaman; functional clarity = lapisan safety/bisnis.

## 1. Prinsip UX
- **Satu pintu Quartermaster.** User tak memilih "mode chat" vs "mode quest" — Quartermaster yang merutekan (lihat `01`). Chat ringan terasa seperti asisten pribadi; quest terasa seperti brief ke CEO.
- **Progressive disclosure (3 tingkat):** Guided Work (pemula) → Operations (power-user) → Technical Control (developer). Default profil user = Technical Control (nyaman penuh teknis), tapi tetap rapi.
- **Artifact before chat:** hasil bernilai muncul sebagai kartu Artifact/Treasure yang bisa direview, bukan tenggelam di scrollback.
- **Lore vs fungsional:** istilah naratif (Quartermaster, Ship, Treasure) di UX; label fungsional di layar safety/cost/policy. Toggle professional mode tersedia (pirate default).

## 2. Layar inti
- **Quartermaster Office (home):** status fleet, priority briefing, recommended actions, dan **satu input chat** (pintu router). Balasan `chat` inline; hasil `report` sebagai kartu; `objective` memunculkan kartu FleetOrderProposal (Approve/Revise/Reject).
- **Mission Board:** Backlog → Ready → Underway → Awaiting Captain → Treasures. Quest = living SOP.
- **Ships → Squads → Crew:** konfigurasi Crew (role, skills persistent, model profile, tool scope, memory scope, status). "Make Me a Squad" (squad composition proposal, lihat re-branding `13`).
- **Engine Room / Crow's Nest:** telemetry (RAM/CPU/health/provider), hasil cabang `engine_room`.
- **Treasury:** biaya provider BYOK **transparan read-only**; Timber = kapasitas (jumlah Ship/Crew/Voyage), bukan kredit.
- **Captain's Approval queue:** aksi high-impact + FleetOrderProposal + MemoryProposal (learning-by-consent).

## 3. Interaksi Quartermaster (dari `01`)
```
[ Tanya atau beri perintah ke Quartermaster … ]
  "apa itu Treasure?"        → jawaban inline (chat)
  "RAM engine berapa?"       → kartu Engine Room (engine_room)
  "ringkas semua ship"       → kartu Fleet Report (report)
  "siapkan rilis v1.4"       → kartu FleetOrderProposal [Approve] [Revise] [Reject] (objective)
```
Saat confidence rendah → Quartermaster bertanya balik (chip): `[ Buat quest baru ]` `[ Pertanyaan biasa ]`.

## 4. Visual direction
- Maritime/exploration theme (lihat `fundamental/design/05-visual-direction`); dark-mode-safe (token tema, bg+text selalu berpasangan — selaras steering web-ts).
- Zero loot-box/XP-authority/streak-manipulatif (gamifikasi sehat saja: quest progress, discovery, treasure, logbook milestone).

## 5. Aksesibilitas
Elemen semantik + keyboard-reachable + aria; kontras cukup di kedua tema. (Detail di steering `galleon-web-ts.md`.)
