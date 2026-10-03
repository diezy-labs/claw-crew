# RF-D Principle Review Checklist (Gateway)

- **Status:** Siap pakai — berlaku untuk SETIAP PR RF-D1 (orchestrator breakdown) dan RF-D2 (channel crate-split lanjutan)
- **Author:** squad-lead
- **Tanggal:** 2026-10-03
- **Relasi:** Melengkapi `rf-c-principle-review-checklist` (artifact existing) — gate umum (SSOT, build-hijau-dua-tempat, no-unwrap-baru, test-count-match, satu-PR-satu-scope, CI-hijau) TETAP BERLAKU dari situ. Dokumen ini menambah gate KHUSUS RF-D yang tidak ada di checklist RF-C.

## Gate D-1: Orchestrator breakdown adalah FILE-SPLIT, bukan crate-split

Berlaku khusus untuk PR RF-D1. Orchestrator secara desain tetap satu crate (`clawcrew-channels`) — tujuannya memecah `mod.rs` jadi submodule, BUKAN memindahkan ke crate baru.
- [ ] PR TIDAK membuat `Cargo.toml` baru atau menambah workspace member.
- [ ] Setiap submodule baru (`registration.rs`, `dispatch.rs`, `lifecycle.rs`, dst) punya SATU tanggung jawab yang bisa dijelaskan 1 kalimat di doc-comment modul.
- [ ] Visibility (`pub(super)`/`pub(crate)`) dipertahankan sesuai asal — PR TIDAK diam-diam mengubah apa yang bisa diakses dari luar crate (itu behavior change, bukan file-split murni).

**Reject jika:** PR mengubah orchestrator jadi crate terpisah tanpa spec RF-D3 + sign-off user eksplisit (lihat catatan RF-D3 di dokumen strategi — itu one-way-door, bukan task RF-D biasa).

## Gate D-2: Governance-coupling check WAJIB dijalankan ULANG oleh Principle, bukan dipercaya dari klaim PR

Setiap PR RF-D2 (channel crate-split) HARUS menyertakan di description hasil:
```
grep 'feature = "channel-<name>"' crates/clawcrew-channels/src/util.rs
```
- [ ] Principle re-run grep ini SENDIRI di worktree (jangan percaya screenshot/klaim di PR description).
- [ ] Kalau hasil grep KOSONG → lanjut gate berikutnya, aman.
- [ ] Kalau hasil grep ADA tapi fungsi yang dipakai adalah helper teknis generik (`ensure_success`, `read_response_body_limited`, atau fungsi serupa yang murni HTTP/parsing, tidak menyentuh `PendingApproval`/`resolve_pending_approval`/approval token) → aman, boleh lanjut. Catat di review log fungsi mana yang shared.
- [ ] Kalau hasil grep ADA dan fungsi yang dipakai adalah governance/approval primitive → **REJECT KERAS**. Channel itu kemungkinan Tier A (kasus matrix, 2026-10-03). PR harus ditahan sampai Lead mengevaluasi ulang tier di ADR-001, BUKAN diselesaikan dengan duplikasi logic atau workaround di PR itu sendiri.

**Reject jika:** PR tidak menyertakan hasil grep ini sama sekali di description, atau Principle re-run sendiri dan hasilnya berbeda dari klaim PR.

## Gate D-3: Transport-class harus sesuai pilihan native-crate vs WASI-plugin

- [ ] Kalau PR adalah RF-D2 (native crate-split): konfirmasi channel memang butuh socket persistent/native crypto/HTTP-outbound-aktif (baca kode, bukan asumsi dari nama channel). Channel yang sebenarnya inbound-only webhook seharusnya masuk RF-C2 (WASI plugin), bukan RF-D2 — PR yang salah jalur ini REJECT dengan arahan pindah jalur, bukan diteruskan.
- [ ] Untuk voice-stack (RF-D2.7 spesifik): konfirmasi PR membuat SATU crate `galleon-channel-voice` dengan 5 submodule, BUKAN 5 crate terpisah. PR yang memecah jadi 5 crate adalah over-fragmentasi — REJECT, minta gabung jadi satu crate.

**Reject jika:** channel inbound-only dipaksa jadi native crate (kehilangan manfaat hot-load tanpa alasan), atau voice-stack dipecah lebih dari 1 crate.

## Gate D-4: Deteksi over-fragmentasi (ukuran minimum untuk crate-split)

- [ ] Channel yang di-crate-split di bawah ~2000 baris DAN tidak menarik dependency unik yang berat (cek `Cargo.toml` crate baru — kalau dependency-nya cuma `clawcrew-api`+`async-trait`+`tokio` dasar yang semua channel lain juga pakai, itu TIDAK memberi manfaat compile nyata) → tanyakan ke Backend di review comment: "channel ini kecil dan dependency-nya ringan, apa manfaat compile konkretnya dibanding tetap di monolith?" Kalau jawaban tidak konkret → REJECT, sarankan tetap di monolith (cukup RF-A file-split kalau perlu rapi).
- [ ] Pengecualian: kalau channel kecil tapi SENGAJA diisolasi untuk alasan non-compile (misal keamanan, platform-specific seperti macOS-only, kemudahan hot-swap) — itu alasan valid, dokumentasikan di PR description, APPROVE.

**Reject jika:** crate-split diajukan murni karena "supaya terlihat modular" tanpa manfaat compile atau alasan isolasi konkret.

## Gate D-5: Peringatan proses — subagent tidak boleh baca file besar secara chunk manual

- [ ] Kalau PR RF-D1 (orchestrator, file 17k+ baris) menunjukkan tanda proses yang tidak efisien (riwayat kerja menyebutkan baca file berkali-kali dengan range kecil, atau nested-spawn subagent untuk "membantu baca"), itu bukan alasan reject KODE-nya (hasil akhir yang dinilai), tapi catat di review log sebagai feedback proses ke Lead/Backend untuk sesi berikutnya — lihat insiden Backend 2026-10-03 (52 menit, nol progress nyata, karena pola ini).

## Ringkasan urutan review PR RF-D

1. Jalankan checklist RF-C existing dulu (SSOT, build-hijau-dua-tempat, no-unwrap, test-count-match, satu-PR-satu-scope, CI-hijau).
2. Tambahkan Gate D-1 s/d D-4 di atas sesuai jenis PR (D1 untuk orchestrator, D2-D4 untuk channel crate-split).
3. D-5 adalah catatan proses, tidak memengaruhi APPROVE/REJECT kode.
4. APPROVE hanya kalau SEMUA gate relevan (RF-C + RF-D yang applicable) lulus.
