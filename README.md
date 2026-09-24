# rust-cbt-client

Aplikasi desktop **CBT (Computer Based Test)** untuk titik ujian, dibangun dengan **Rust + Tauri 2** dan React.
Pasangannya adalah server pusat di repo [`rust-cbt-master`](https://github.com/gryfklapryd/rust-cbt-master).

Satu aplikasi dengan **dua mode**, dipilih saat pengaturan awal:

- **Server lokal**: satu komputer per titik ujian, dipegang proktor. Hanya komputer ini yang terhubung ke server pusat
  (memegang kode + secret lokasi), menyimpan data ujian semua peserta, dan menyediakan dasbor proktor.
- **PC peserta**: komputer tempat peserta ujian. Cukup diisi **alamat IP server lokal**, lalu disetujui proktor.

```
 Server pusat  <--- internet (sebelum & sesudah ujian) --->  Server lokal (proktor)
                                                                 |  LAN, tanpa internet
                                                    +------------+------------+
                                                 PC peserta   PC peserta   PC peserta
```

Alur kerja di titik ujian:

1. **Sebelum ujian (online)**: proktor login di server lokal dengan akun pusatnya, lalu mengunduh paket ujian
   (peserta, soal, jadwal, media) dari server pusat.
2. **Saat ujian (LAN)**: PC peserta mengambil soal dari server lokal. Peserta login dengan nomor peserta, password, dan token sesi.
   Setiap jawaban dikirim ke server lokal saat itu juga. Internet tidak diperlukan.
3. **Sesudah ujian (online)**: server lokal mengirim hasil semua peserta (dan log aksi proktor) ke server pusat, lalu dinilai di sana.

## Fitur

- Mendukung **15 jenis soal** (pilihan ganda, kompleks, benar/salah, benar/salah majemuk, isian singkat, isian angka,
  uraian, menjodohkan, mengurutkan, isian rumpang, pengelompokan, hotspot gambar, pilih teks, matriks/Likert, unggah berkas),
  memakai komponen yang sama dengan pratinjau di panel admin server.
- **Akun proktor dari server pusat**: admin menugaskan proktor ke lokasi; proktor login di server lokal (juga saat tidak ada
  internet, diverifikasi dengan hash argon2id yang disinkron). Semua aksi proktor dicatat dan dikirim ke pusat.
- **Dasbor proktor**: token sesi, status setiap peserta (belum login, sedang ujian, selesai, PC terputus), soal terjawab,
  sisa waktu, pelanggaran. Aksi: tambah waktu, hentikan, buka kunci, izinkan pindah komputer, hapus data ujian.
- **Pendaftaran PC peserta**: PC peserta mendaftar dengan token perangkat acak; proktor mencocokkan kode 4 digit lalu menyetujui.
  PC asing di jaringan tidak bisa mengambil soal atau mengirim jawaban.
- **Tahan gangguan jaringan**: bila kabel/WiFi PC peserta putus, peserta tetap mengerjakan. Jawaban diantrekan di PC dan dikirim
  berurutan saat tersambung lagi. Bila PC rusak, proktor mengizinkan pindah komputer dan peserta melanjutkan dari jawaban terakhir.
- **Satu PC untuk banyak peserta secara bergantian** (mis. sesi pagi dan siang). PC peserta hanya menyimpan salinan jawaban
  selama ujian berjalan atau masih ada yang belum terkirim; setelah ujian selesai dan semua terkirim ke server lokal, salinan
  jawaban dan berkas lampiran dihapus dari PC tersebut.
- **Kunci jawaban tidak pernah ada di titik ujian**; penilaian dilakukan di server pusat.
- **Pengacakan** soal (termasuk ambil acak N soal per bagian) dan opsi, deterministik per peserta.
- **Timer dijaga server lokal**; ujian dikumpulkan otomatis saat waktu habis.
- **Mode terkunci** di PC peserta: layar penuh, selalu di atas, jendela tidak bisa ditutup selama ujian, tombol muat ulang/cetak
  diblokir; keluar dari layar ujian dicatat sebagai pelanggaran dan ujian dihentikan bila melewati batas.
- Media soal disalin ke PC peserta (protokol `cbtasset://`) dan diverifikasi SHA-256.

## Struktur

```
src/                  Frontend React
  pages/Setup.tsx     Pengaturan awal: pilih mode, koneksi
  pages/server/       Dasbor proktor server lokal (pemantauan, PC peserta, paket, hasil, log, pengaturan)
  pages/Home.tsx      Login peserta (PC peserta); Pairing.tsx, ParticipantMenu.tsx, Exam.tsx, Finished.tsx
packages/shared       Salinan kontrak dari rust-cbt-master (skema soal & format sinkronisasi)
packages/question-ui  Salinan komponen tampilan soal dari rust-cbt-master
src-tauri/
  src/core/           Logika inti tanpa ketergantungan GUI (bisa dites langsung)
    api.rs            Klien HTTP server lokal ke /api/sync/* server pusat
    sync.rs           Unduh paket & media, akun proktor, kirim hasil, lampiran, log proktor
    exam.rs           Login peserta, sesi, jawaban, pelanggaran, pengumpulan, aksi proktor
    proctor.rs        Akun proktor, PC peserta terdaftar, log aksi proktor, data pemantauan
    lan.rs            API LAN server lokal untuk PC peserta (axum, /lan/v1/*)
    lan_client.rs     Klien HTTP PC peserta ke API LAN
    participant.rs    PC peserta: salinan sesi, antrean kirim saat terputus
    db.rs             Penyimpanan lokal SQLite
    shuffle.rs        Pengacakan deterministik soal & opsi
    crypto.rs         Verifikasi argon2id, token perangkat, SHA-256
  src/commands.rs     Command Tauri untuk frontend
  src/lib.rs          Setup aplikasi, API LAN, protokol cbtasset, cegah tutup jendela, sinkron otomatis
  tests/              Fixture paket asli, tes LAN (server lokal + PC peserta), tes end-to-end dengan server pusat
```

Kode di `packages/` disalin dari repo server supaya kedua aplikasi memakai kontrak yang sama. Jangan ubah di sini;
perbarui di `rust-cbt-master` lalu jalankan `scripts/sync-shared.sh ../rust-cbt-master`.

## Pemakaian di titik ujian

**Di server pusat (admin):** buat titik ujian (catat kode + secret), buat akun proktor (peran `proctor`) dan tugaskan ke
lokasi di **Titik Ujian → Proktor**, lalu terbitkan jadwal.

**Server lokal (sekali, oleh proktor/teknisi):**

1. Pasang aplikasi (installer `.msi`/`.exe` Windows atau `.deb`/`.AppImage` Linux dari artifact CI) di komputer proktor.
2. Pilih **Server lokal**, isi alamat server pusat, kode lokasi, secret, nama server, dan port LAN (bawaan `8787`).
3. Tekan **Perbarui akun proktor dari server pusat**, lalu login dengan akun proktor.
4. Buka **PC peserta**: catat alamat yang tertera (mis. `192.168.1.10:8787`). Bila Windows menanyakan izin firewall, izinkan
   untuk jaringan Private. Beri komputer ini IP tetap (atau reservasi DHCP) agar alamatnya tidak berubah.
5. Buka **Paket ujian** → **Unduh** jadwal yang tampil. Ulangi bila admin menerbitkan versi paket baru.

**Setiap PC peserta (sekali):**

1. Pasang aplikasi, pilih **PC peserta**, isi alamat IP server lokal (port boleh tidak ditulis) dan nama komputer.
2. Layar menampilkan kode 4 digit. Di server lokal, menu **PC peserta**, cocokkan kode lalu **Setujui** (atau **Setujui semua**).

**Saat ujian:** proktor membacakan **token sesi** yang tampil di tab **Pemantauan**. Peserta login di PC peserta. Proktor memantau
status tiap peserta dan bisa menambah waktu, menghentikan, membuka kunci, atau mengizinkan pindah komputer.

**Sesudah ujian:** hasil terkirim otomatis tiap menit saat server lokal online, atau lewat **Hasil & sinkronisasi** →
**Kirim hasil sekarang**. Pastikan **Belum terkirim** bernilai 0 sebelum server lokal dimatikan.

Catatan:

- Menu proktor di PC peserta (ubah alamat server, tutup aplikasi) dibuka dengan login proktor yang diverifikasi server lokal.
  Selama PC belum disetujui server lokal, alamat server boleh diubah tanpa login (mis. salah ketik IP).
- Mode tidak bisa diganti setelah dipilih. Untuk mengganti, hapus folder data aplikasi (lihat di bawah).
- Jam server lokal menjadi acuan waktu ujian; *Tes koneksi* di Pengaturan memberi peringatan bila selisihnya dengan server pusat
  lebih dari 2 menit.
- Komunikasi LAN memakai HTTP biasa dengan token perangkat. Gunakan jaringan lab yang tidak dipakai umum.

## API LAN (server lokal)

Semua di bawah `http://<ip-server-lokal>:8787/lan/v1`. Kecuali `/info` dan `/pair`, wajib `Authorization: Bearer <token perangkat>`
dari PC yang sudah disetujui. Setiap respons membawa header `x-server-time`. Error: `{ "message": "...", "code": "..." }` dengan
`code` antara lain `unpaired`, `pending`, `revoked`, `moved`, `other_device`, `user`.

| Endpoint | Fungsi |
|---|---|
| `GET /info` | identitas server lokal (kode lokasi, nama, versi, jam) |
| `POST /pair` | daftar / cek status PC (`deviceId`, `name`, `token`) |
| `GET /me` | status PC ini |
| `GET /schedules` | jadwal yang paketnya ada di server lokal |
| `GET /assets/:id` | media soal |
| `POST /login` | login peserta; mengembalikan sesi + daftar media |
| `GET /attempts/:id`, `GET /attempts/:id/state` | sesi lengkap / status ringkas (sisa waktu, status) |
| `POST /attempts/:id/answer`, `/event`, `/submit`, `/attachment` | simpan jawaban, kejadian, kumpulkan, lampiran |
| `POST /proctor/verify` | verifikasi login proktor dari PC peserta |

Aksi yang diantrekan PC peserta membawa `at` (waktu saat dibuat, menurut jam server lokal). Server lokal memakai waktu itu untuk
memeriksa batas waktu, sehingga jawaban yang dibuat sebelum waktu habis tetap diterima walau baru terkirim sesudahnya.

## Pengembangan

Kebutuhan: Rust stabil, Node.js 22, pnpm 10, dan pustaka sistem Tauri
([daftar per OS](https://v2.tauri.app/start/prerequisites/)). Di Ubuntu:

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libsoup-3.0-dev
```

```bash
pnpm install
pnpm app:dev          # jalankan aplikasi dengan hot reload
pnpm app:build        # installer untuk OS saat ini (src-tauri/target/release/bundle)
```

Data lokal tersimpan di folder data aplikasi (`id.cbt.client`), mis. `%APPDATA%\id.cbt.client` di Windows atau
`~/.local/share/id.cbt.client` di Linux: database `cbt.sqlite3`, folder `assets/` (media) dan `attachments/` (berkas jawaban).

Untuk mencoba kedua mode di satu komputer Linux, jalankan instans kedua dengan folder data lain, mis.
`XDG_DATA_HOME=/tmp/pc1 src-tauri/target/debug/cbt-client`. Di Windows, pakai dua akun pengguna Windows atau dua komputer.

### Tes

```bash
cd src-tauri
cargo test                     # tes unit + tes LAN (server lokal & PC peserta lewat HTTP sungguhan)
cargo clippy --all-targets
```

Tes end-to-end melawan server pusat sungguhan (jalankan server `rust-cbt-master` dengan `pnpm db:seed --demo` dan
`INLINE_WORKER=true pnpm dev:server`). Rantainya: server pusat → server lokal → API LAN → PC peserta → server lokal → server pusat.

```bash
CBT_E2E_SERVER=http://localhost:3000 cargo test --test e2e_server -- --nocapture
```

Tes ini mengambil akun proktor, mengunduh paket dan media ke server lokal, mendaftarkan PC peserta, login peserta, menjawab
ke-15 jenis soal (termasuk unggah berkas), mengumpulkan, mengirim hasil dan log proktor, lalu memeriksa nilai yang dihitung
server pusat.
