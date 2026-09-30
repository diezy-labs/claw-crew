# 🚀 Fleet AI — Multi-Platform Deployment & Access Guide

Fleet AI dirancang dengan arsitektur **Self-Hosted Core + Universal Multi-Platform Client**. Anda dapat menjalankannya di satu mesin (laptop lokal, mini PC homelab, server VPS, atau Cloud) dan mengaksesnya secara langsung dari laptop lain, smartphone (iOS / Android), tablet, maupun desktop webview.

---

## 1. Menjalankan secara Lokal / CLI

### Mode Pengembangan (Development):
```bash
# Jalankan server daemon (Express + Vite HMR) pada 0.0.0.0:3000
npm run dev
```

### Mode Produksi (Production):
```bash
# Build frontend bundle
npm run build

# Jalankan daemon production
npm start
```
Server akan mendeteksi seluruh antarmuka jaringan (LAN IP) secara otomatis dan menampilkannya di terminal:
```
======================================================
⚓ Fleet AI Core Daemon is running
   Host: 0.0.0.0 | Port: 3000 | Mode: Production
------------------------------------------------------
📡 Local Access:        http://localhost:3000
📱 LAN / Mobile Access: http://192.168.1.45:3000 (wlan0)
🌐 Ready for Nginx / Cloudflare / Docker / Webview
======================================================
```

---

## 2. Akses dari Smartphone (iOS & Android) via PWA

1. Pastikan HP dan server terhubung ke **jaringan Wi-Fi yang sama**.
2. Buka Fleet AI di komputer Anda, lalu klik tombol **"Remote Access / Scan QR"** di header atas (atau buka langsung `http://<IP_KOMPUTER>:3000`).
3. Scan **QR Code** yang muncul menggunakan kamera smartphone Anda.
4. **Instal sebagai Aplikasi Native (PWA)**:
   - **iPhone / iPad (Safari)**: Tekan tombol **Share** (kotak panah ke atas) &rarr; pilih **"Add to Home Screen"**.
   - **Android (Chrome)**: Tekan menu tiga titik &rarr; pilih **"Install App"** atau **"Add to Home screen"**.
5. Fleet AI kini berjalan di HP Anda sebagai aplikasi mandiri berlayar penuh tanpa URL bar, lengkap dengan kontrol sentuh untuk audio, obrolan, dan persetujuan tugas agen.

---

## 3. Menjalankan Menggunakan Docker

Tersedia file `Dockerfile` dan `docker-compose.yml` multi-arch:

```bash
# Menjalankan container di background
docker compose up -d

# Memeriksa status dan log
docker compose logs -f fleet-ai
```

---

## 4. Reverse Proxy dengan Nginx (Domain & SSL)

Jika Anda ingin mengakses Fleet AI dari luar rumah via domain publik (misal: `https://fleet.yourdomain.com`):

1. Salin template `nginx.conf` ke server Anda:
   ```bash
   sudo cp nginx.conf /etc/nginx/sites-available/fleet-ai
   sudo ln -s /etc/nginx/sites-available/fleet-ai /etc/nginx/sites-enabled/
   ```
2. Pasang sertifikat SSL gratis via Certbot (Let's Encrypt):
   ```bash
   sudo certbot --nginx -d fleet.yourdomain.com
   ```
3. Restart Nginx:
   ```bash
   sudo systemctl restart nginx
   ```

---

## 5. Integrasi Model Lokal (Ollama)

Fleet AI menyertakan **Reverse-Proxy Bridge** ke Ollama lokal:
- Secara default, daemon meneruskan permintaan ke `http://127.0.0.1:11434`.
- Perangkat lain (misal HP atau laptop lain) tidak akan terhalang oleh pembatasan *CORS* browser karena semua request dijembatani secara aman oleh server daemon.
