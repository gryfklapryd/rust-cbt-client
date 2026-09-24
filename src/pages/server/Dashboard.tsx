import { useCallback, useEffect, useState } from "react";
import { Alert, Badge, Button } from "../../components/ui";
import { fmtDateTime } from "../../lib/format";
import { errorText, ipc, type ConfigView, type Proctor, type ProctorLogRow } from "../../lib/ipc";
import { ServerConfigForm } from "../Setup";
import { DevicesTab } from "./Devices";
import { MonitorTab } from "./Monitor";
import { PackagesTab, ResultsTab } from "./Packages";

type Tab = "monitor" | "devices" | "packages" | "results" | "log" | "settings";

export type Notify = (m: { tone: "success" | "danger" | "info"; text: string } | null) => void;

const TABS: [Tab, string][] = [
  ["monitor", "Pemantauan"],
  ["devices", "PC peserta"],
  ["packages", "Paket ujian"],
  ["results", "Hasil & sinkronisasi"],
  ["log", "Log proktor"],
  ["settings", "Pengaturan"],
];

export const ACTION_LABELS: Record<string, string> = {
  login: "login",
  logout: "logout",
  device_approve: "setujui PC peserta",
  device_revoke: "cabut PC peserta",
  attempt_reset_device: "izinkan pindah komputer",
  attempt_extra_time: "tambah waktu",
  attempt_terminate: "hentikan ujian peserta",
  attempt_unlock: "buka kunci peserta",
  attempt_delete: "hapus attempt",
  package_download: "unduh paket",
  results_upload: "kirim hasil",
  results_export: "ekspor hasil",
  settings_change: "ubah pengaturan",
};

/** Dasbor proktor di server lokal. */
export function ServerDashboard({
  config,
  proctor,
  onConfigChanged,
  onLogout,
}: {
  config: ConfigView;
  proctor: Proctor;
  onConfigChanged: (c: ConfigView) => void;
  onLogout: () => void;
}) {
  const [tab, setTab] = useState<Tab>("monitor");
  const [message, setMessage] = useState<{ tone: "success" | "danger" | "info"; text: string } | null>(null);

  // Sesi proktor berakhir: kembali ke layar login.
  const notify = useCallback<Notify>(
    (m) => {
      if (m?.tone === "danger" && m.text.includes("Sesi proktor berakhir")) {
        onLogout();
        return;
      }
      setMessage(m);
    },
    [onLogout],
  );

  const logout = async () => {
    await ipc.proctorLogout().catch(() => {});
    onLogout();
  };

  return (
    <div className="operator">
      <header className="operator-header">
        <div className="brand"><span className="brand-logo">CBT</span> Server lokal · {config.siteCode}</div>
        <nav className="tabs">
          {TABS.map(([id, label]) => (
            <button key={id} type="button" className={`tab${tab === id ? " is-active" : ""}`} onClick={() => { setTab(id); setMessage(null); }}>
              {label}
            </button>
          ))}
        </nav>
        <div className="actions header-user">
          <span className="small muted">{proctor.name} ({proctor.username})</span>
          <Button onClick={() => void logout()}>Keluar</Button>
          <Button
            variant="danger"
            onClick={() => {
              if (window.confirm("Tutup aplikasi server lokal? PC peserta tidak bisa ujian selama server lokal mati."))
                ipc.quitApp().catch((e) => notify({ tone: "danger", text: errorText(e) }));
            }}
          >
            Tutup aplikasi
          </Button>
        </div>
      </header>
      <main className="operator-body">
        {message ? <Alert tone={message.tone}>{message.text}</Alert> : null}
        {tab === "monitor" ? <MonitorTab notify={notify} /> : null}
        {tab === "devices" ? <DevicesTab notify={notify} /> : null}
        {tab === "packages" ? <PackagesTab notify={notify} /> : null}
        {tab === "results" ? <ResultsTab notify={notify} /> : null}
        {tab === "log" ? <LogTab notify={notify} /> : null}
        {tab === "settings" ? <SettingsTab config={config} onConfigChanged={onConfigChanged} notify={notify} /> : null}
      </main>
    </div>
  );
}

function LogTab({ notify }: { notify: Notify }) {
  const [rows, setRows] = useState<ProctorLogRow[] | null>(null);
  useEffect(() => {
    ipc.proctorLog().then(setRows).catch((e) => notify({ tone: "danger", text: errorText(e) }));
  }, [notify]);
  return (
    <section className="card">
      <h2>Log aksi proktor</h2>
      <p className="muted small">Semua aksi proktor di lokasi ini dicatat dan dikirim ke server pusat bersama hasil ujian.</p>
      {!rows ? <p className="muted">Memuat…</p> : !rows.length ? <p className="muted">Belum ada.</p> : (
        <table className="table">
          <thead><tr><th>Waktu</th><th>Proktor</th><th>Aksi</th><th>Rincian</th><th>Terkirim</th></tr></thead>
          <tbody>
            {rows.map((r) => (
              <tr key={r.id}>
                <td className="small">{fmtDateTime(r.at)}</td>
                <td className="mono small">{r.username}</td>
                <td>{ACTION_LABELS[r.action] ?? r.action}</td>
                <td className="small mono">
                  {r.attemptId ? <div>attempt {r.attemptId.slice(0, 8)}</div> : null}
                  {r.data && Object.keys(r.data).length ? JSON.stringify(r.data) : null}
                </td>
                <td>{r.synced ? <Badge tone="success">ya</Badge> : <Badge tone="warning">belum</Badge>}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

function SettingsTab({ config, onConfigChanged, notify }: { config: ConfigView; onConfigChanged: (c: ConfigView) => void; notify: Notify }) {
  const [info, setInfo] = useState<{ version: string; dataDir: string } | null>(null);
  const [testing, setTesting] = useState(false);
  useEffect(() => {
    ipc.appInfo().then(setInfo).catch(() => {});
  }, []);
  const test = async () => {
    setTesting(true);
    try {
      const r = await ipc.testConnection();
      const skew = Math.round((new Date(r.serverTime).getTime() - Date.now()) / 1000);
      notify({
        tone: Math.abs(skew) > 120 ? "info" : "success",
        text: `Terhubung sebagai ${r.site.code} (${r.site.name}). Akun proktor diperbarui.${Math.abs(skew) > 120 ? ` Jam komputer ini selisih ${skew} detik dari server pusat, sesuaikan jam komputer!` : ""}`,
      });
    } catch (err) {
      notify({ tone: "danger", text: errorText(err) });
    } finally {
      setTesting(false);
    }
  };
  return (
    <div className="stack">
      <section className="card">
        <div className="card-header">
          <h2>Koneksi server pusat</h2>
          <Button loading={testing} onClick={() => void test()}>Tes koneksi</Button>
        </div>
        <p className="muted small">
          Tes koneksi memakai alamat yang sudah disimpan: <span className="mono">{config.serverUrl ?? "-"}</span>. Tekan Simpan dulu bila
          alamat di bawah diubah. Mengganti port LAN membuat PC peserta perlu diisi alamat baru.
        </p>
        <ServerConfigForm config={config} onSaved={onConfigChanged} />
      </section>
      <section className="card">
        <h2>Tentang</h2>
        <dl className="kv">
          <dt>Versi aplikasi</dt><dd>{info?.version ?? "-"}</dd>
          <dt>Peran</dt><dd>Server lokal</dd>
          <dt>ID perangkat</dt><dd className="mono">{config.deviceId}</dd>
          <dt>Folder data</dt><dd className="mono small">{info?.dataDir ?? "-"}</dd>
        </dl>
      </section>
    </div>
  );
}
