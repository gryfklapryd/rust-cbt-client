import { useEffect, useState } from "react";
import { Alert, Button } from "../components/ui";
import { errorText, ipc, type ConfigView, type LinkStatus, type Proctor } from "../lib/ipc";
import { ParticipantConfigForm } from "./Setup";

/** Menu proktor di PC peserta: status koneksi, alamat server lokal, tutup aplikasi. */
export function ParticipantMenu({
  config,
  proctor,
  onConfigChanged,
  onClose,
}: {
  config: ConfigView;
  proctor: Proctor;
  onConfigChanged: (c: ConfigView) => void;
  onClose: () => void;
}) {
  const [link, setLink] = useState<LinkStatus | null>(null);
  const [info, setInfo] = useState<{ version: string; dataDir: string } | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const load = () => ipc.linkStatus().then(setLink).catch((e) => setError(errorText(e)));
    void load();
    ipc.appInfo().then(setInfo).catch(() => {});
    const t = setInterval(load, 5_000);
    return () => clearInterval(t);
  }, []);

  const close = async () => {
    await ipc.proctorLogout().catch(() => {});
    onClose();
  };

  return (
    <div className="operator">
      <header className="operator-header">
        <div className="brand"><span className="brand-logo">CBT</span> Proktor · {config.deviceName ?? "PC peserta"}</div>
        <div className="actions header-user">
          <span className="small muted">{proctor.name} ({proctor.username})</span>
          <Button onClick={() => void close()}>Kembali ke layar ujian</Button>
          <Button
            variant="danger"
            onClick={() => {
              if (window.confirm("Tutup aplikasi di komputer ini?")) ipc.quitApp().catch((e) => setError(errorText(e)));
            }}
          >
            Tutup aplikasi
          </Button>
        </div>
      </header>
      <main className="operator-body stack">
        <Alert>{error}</Alert>
        <section className="card">
          <h2>Koneksi ke server lokal</h2>
          {link ? (
            link.connected ? (
              <Alert tone="success">Terhubung ke <span className="mono">{config.lanUrl}</span>.{link.pending ? ` ${link.pending} data sedang dikirim.` : " Semua data sudah terkirim."}</Alert>
            ) : (
              <Alert>
                Tidak terhubung ke <span className="mono">{config.lanUrl}</span>.{link.pending ? ` ${link.pending} data menunggu dikirim; jangan hapus data aplikasi.` : ""}
                {link.error ? `\n(${link.error})` : ""}
              </Alert>
            )
          ) : (
            <p className="muted">Memeriksa…</p>
          )}
        </section>
        <section className="card">
          <h2>Pengaturan komputer ini</h2>
          <p className="muted small">Mengganti alamat server lokal membuat komputer ini perlu disetujui lagi di server tersebut.</p>
          <ParticipantConfigForm config={config} onSaved={onConfigChanged} />
        </section>
        <section className="card">
          <h2>Tentang</h2>
          <dl className="kv">
            <dt>Versi aplikasi</dt><dd>{info?.version ?? "-"}</dd>
            <dt>Peran</dt><dd>PC peserta</dd>
            <dt>ID perangkat</dt><dd className="mono">{config.deviceId}</dd>
            <dt>Folder data</dt><dd className="mono small">{info?.dataDir ?? "-"}</dd>
          </dl>
        </section>
      </main>
    </div>
  );
}
