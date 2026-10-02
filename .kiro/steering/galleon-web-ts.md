---
title: Galleon web-2 — konvensi TypeScript React
inclusion: fileMatch
fileMatchPattern: "web-2/**/*.{ts,tsx}"
---

# web-2 (React 19 + TS + Vite) — konvensi

Loads saat menyentuh `.ts`/`.tsx` di `web-2/`. UI tier: **zero backend logic** — hanya render + panggil engine/IPC. Test: Vitest + React Testing Library (`src/__tests__/`).

## Boundary (STRICT)
- **Tidak ada logika bisnis/domain di UI.** Komponen memanggil `apiClient` (→ engine HTTP :9090 atau Tauri IPC); jangan hitung/putuskan fakta fleet di client. Logika host-only (network/Ollama/proses) ada di `web-2/server.ts`, bukan di komponen React.
- Satu klien API: `src/utils/apiClient.ts`. Komponen/stores **tidak** `fetch()` langsung ke `/api/...` — tambah method di `apiClient` (lihat `RemoteAccessModal` yang `fetch` langsung sebagai anti-pola yang harus dirapikan).
- `apiClient` sudah punya pola Tauri-first + HTTP fallback (`isTauri()` → `invoke(...)` lalu `fetch`). Ikuti pola itu untuk endpoint baru.

## TypeScript
- **Hindari `any`.** Tipe domain hidup di `src/types/index.ts` — pakai & perluas di sana, jangan `Promise<any>`. Beri tipe retur eksplisit pada method `apiClient`.
- `interface` untuk bentuk objek/props; `type` untuk union/alias. `unknown` + narrowing untuk data eksternal, bukan `any`.
- Jangan non-null assertion `!` kecuali invariant jelas; prefer optional chaining `?.` + default.
- Import type-only: `import type { Foo }` untuk tipe yang tak dipakai runtime.

## React 19
- Function component + hooks saja. Hormati rules-of-hooks (jangan panggil kondisional). `useEffect` dep array lengkap; cleanup listener/timeout di return.
- Derive state saat render; jangan duplikat state yang bisa dihitung. Angkat state seperlunya.
- State global via **zustand `useFleetStore`** (`src/store/fleetStore.ts`) — jangan bikin store paralel untuk fakta yang sama (SSOT). Selector sempit agar re-render minim.
- `key` stabil pada list (bukan index bila item bisa reorder). Jangan mutasi props/state; buat objek/array baru.
- Komponen diorganisir `components/{common,features,layout}`; satu komponen per file, named export.

## Styling & a11y
- Tailwind utility-first (lihat `index.css`/`themeEngine.ts`). Warna lewat token tema, bukan hex liar; selalu set bg + text bersama (dark-mode safe).
- Interaktif = elemen semantik (`button`, `a`) dengan label; keyboard-reachable; `aria-*` saat perlu.

## Verifikasi
- `tsc --noEmit` bersih (butuh `node_modules` ter-install di `web-2/`). Vitest untuk komponen/store yang diubah. ESLint bila tersedia.
- Hapus `fetch` langsung yang menembak endpoint tak terdaftar (lihat kontrak route di `galleon-architecture.md`).
