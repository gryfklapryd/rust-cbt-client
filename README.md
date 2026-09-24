# rust-cbt-client

Aplikasi desktop **CBT (Computer Based Test)** untuk titik ujian, dibangun dengan **Rust + Tauri 2** dan React.
Pasangannya adalah server pusat di repo [`rust-cbt-master`](https://github.com/gryfklapryd/rust-cbt-master).

Alur kerja di titik ujian:

1. **Sebelum ujian (online)**: operator mengunduh paket ujian dari server pusat: peserta, soal, jadwal, dan media.
2. **Saat ujian (offline)**: peserta login dengan nomor peserta, password, dan token sesi. Ujian berjalan tanpa internet;
   setiap jawaban langsung tersimpan di komputer.
3. **Sesudah ujian (online)**: hasil dikirim ke server pusat (otomatis tiap menit, atau lewat tombol operator), lalu dinilai di sana.

## Fitur

- Mendukung **15 jenis soal** (pilihan ganda, kompleks, benar/salah, benar/salah majemuk, isian singkat, isian angka,
  uraian, menjodohkan, mengurutkan, isian rumpang, pengelompokan, hotspot gambar, pilih teks, matriks/Likert, unggah berkas),
  memakai komponen yang sama dengan pratinjau di panel admin server.
- **Login offline**: password peserta dan token sesi diverifikasi dengan hash argon2id yang ada di paket.
- **Kunci jawaban tidak pernah ada di komputer ujian**; penilaian dilakukan di server pusat.
- **Pengacakan** soal (termasuk ambil acak N soal per bagian) dan opsi, deterministik per peserta sehingga urutan tetap sama
  bila aplikasi dibuka ulang.
- **Timer dijaga backend** (bukan di tampilan), batas akhir mengikuti durasi ujian dan akhir jadwal; ujian dikumpulkan otomatis saat waktu habis.
- **Tahan gangguan**: jawaban disimpan ke SQLite setiap berubah; bila aplikasi tertutup atau listrik padam, peserta login lagi
  dan melanjutkan dengan sisa waktu yang benar.
- **Mode terkunci**: layar penuh, selalu di atas, jendela tidak bisa ditutup selama ujian, tombol muat ulang/cetak diblokir;
  keluar dari layar ujian dicatat sebagai pelanggaran dan ujian dihentikan bila melewati batas yang diatur di server.
- **Menu operator** (dilindungi PIN): unduh/perbarui paket, pantau peserta di komputer ini, kirim hasil, riwayat pengiriman,
  pengaturan koneksi.
- Media soal disajikan dari berkas lokal (protokol `cbtasset://`) dan diverifikasi SHA-256 saat diunduh.

## Struktur

```
src/                  Frontend React (layar pengaturan, login, operator, ujian, selesai)
packages/shared       Salinan kontrak dari rust-cbt-master (skema soal & format sinkronisasi)
packages/question-ui  Salinan komponen tampilan soal dari rust-cbt-master
src-tauri/
  src/core/           Logika inti tanpa ketergantungan GUI (bisa dites langsung)
    api.rs            Klien HTTP ke /api/sync/* server pusat
    db.rs             Penyimpanan lokal SQLite
    exam.rs           Login offline, sesi, jawaban, pelanggaran, pengumpulan
    shuffle.rs        Pengacakan deterministik soal & opsi
    sync.rs           Unduh paket & media, kirim hasil & lampiran, status batch
    crypto.rs         Verifikasi argon2id, SHA-256
  src/commands.rs     Command Tauri untuk frontend
  src/lib.rs          Setup aplikasi, protokol cbtasset, cegah tutup jendela, sinkron otomatis
  tests/              Fixture paket asli dan tes end-to-end terhadap server sungguhan
```

Kode di `packages/` disalin dari repo server supaya kedua aplikasi memakai kontrak yang sama. Jangan ubah di sini;
perbarui di `rust-cbt-master` lalu jalankan `scripts/sync-shared.sh ../rust-cbt-master`.

## Pemakaian di titik ujian

1. Pasang aplikasi (installer `.msi`/`.exe` Windows atau `.deb`/`.AppImage` Linux dari artifact CI).
2. Saat pertama dibuka, isi **alamat server pusat**, **kode lokasi** dan **secret** (dibuat admin di menu *Titik Ujian*
   panel admin), buat **PIN operator**, dan beri nama komputer (mis. `LAB1-PC07`).
3. Buka **Operator** (pojok kanan bawah) → *Paket ujian* → **Unduh** jadwal yang tampil. Ulangi bila admin menerbitkan versi paket baru.
4. Peserta login di layar utama. Token sesi dibacakan pengawas bila jadwal memakai token.
5. Hasil terkirim otomatis saat komputer online; bisa juga lewat *Operator* → *Hasil & sinkronisasi* → **Kirim hasil sekarang**.
   Pastikan kolom *Belum terkirim* bernilai 0 sebelum komputer dimatikan.

Catatan: setiap komputer ujian berdiri sendiri (mengunduh paket dan mengirim hasil masing-masing). Seorang peserta hanya bisa
ujian di satu komputer; bila perlu pindah komputer, admin menghapus attempt di server dan operator menekan *Reset* di komputer lama.
Jam komputer harus benar; *Tes koneksi* di menu operator memberi peringatan bila selisih jam dengan server lebih dari 2 menit.

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

### Tes

```bash
cd src-tauri
cargo test                     # tes unit (memakai paket asli dari server sebagai fixture)
cargo clippy --all-targets
```

Tes end-to-end melawan server pusat sungguhan (jalankan server `rust-cbt-master` dengan `pnpm db:seed --demo` dan
`INLINE_WORKER=true pnpm dev:server`):

```bash
CBT_E2E_SERVER=http://localhost:3000 cargo test --test e2e_server -- --nocapture
```

Tes ini mengunduh paket dan media, login peserta, menjawab ke-15 jenis soal (termasuk unggah berkas), mengumpulkan,
mengirim hasil, lalu memeriksa nilai yang dihitung server.
