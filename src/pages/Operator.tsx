import { useCallback, useEffect, useState } from "react";
import { Alert, Badge, Button, StatusBadge } from "../components/ui";
import { errorText, ipc, type AttemptRow, type ConfigView, type LocalSchedule, type RemoteSchedule, type SyncStatus } from "../lib/ipc";
import { fmtDateTime } from "../lib/format";
import { ConfigForm } from "./Setup";

type Tab = "packages" | "results" | "settings";

export function OperatorPage({ config, onConfigChanged, onClose }: { config: ConfigView; onConfigChanged: (c: ConfigView) => void; onClose: () => void }) {
  const [tab, setTab] = useState<Tab>("packages");
  const [message, setMessage] = useState<{ tone: "success" | "danger" | "info"; text: string } | null>(null);

  const close = async () => {
    await ipc.lockOperator().catch(() => {});
    onClose();
  };

  return (
    <div className="operator">
      <header className="operator-header">
        <div className="brand"><span className="brand-logo">CBT</span> Operator · {config.siteCode}</div>
        <nav className="tabs">
          {([
            ["packages", "Paket ujian"],
            ["results", "Hasil & sinkronisasi"],
            ["settings", "Pengaturan"],
          ] as const).map(([id, label]) => (
            <button key={id} type="button" className={`tab${tab === id ? " is-active" : ""}`} onClick={() => setTab(id)}>
              {label}
            </button>
          ))}
        </nav>
        <div className="actions">
          <Button onClick={() => void close()}>Kembali ke layar ujian</Button>
          <Button
            variant="danger"
            onClick={() => {
              if (window.confirm("Tutup aplikasi?")) ipc.quitApp().catch((e) => setMessage({ tone: "danger", text: errorText(e) }));
            }}
          >
            Tutup aplikasi
          </Button>
        </div>
      </header>
      <main className="operator-body">
        {message ? <Alert tone={message.tone}>{message.text}</Alert> : null}
        {tab === "packages" ? <PackagesTab notify={setMessage} /> : null}
        {tab === "results" ? <ResultsTab notify={setMessage} /> : null}
        {tab === "settings" ? <SettingsTab config={config} onConfigChanged={onConfigChanged} notify={setMessage} /> : null}
      </main>
    </div>
  );
}

type Notify = (m: { tone: "success" | "danger" | "info"; text: string } | null) => void;

function PackagesTab({ notify }: { notify: Notify }) {
  const [remote, setRemote] = useState<RemoteSchedule[] | null>(null);
  const [local, setLocal] = useState<LocalSchedule[]>([]);
  const [loading, setLoading] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);

  const loadLocal = useCallback(() => ipc.localSchedules().then(setLocal), []);
  const loadRemote = useCallback(async () => {
    setLoading(true);
    try {
      setRemote(await ipc.remoteSchedules());
    } catch (err) {
      notify({ tone: "danger", text: errorText(err) });
    } finally {
      setLoading(false);
    }
  }, [notify]);

  useEffect(() => {
    void loadLocal();
    void loadRemote();
  }, [loadLocal, loadRemote]);

  const download = async (id: string) => {
    setBusyId(id);
    notify(null);
    try {
      const r = await ipc.downloadSchedule(id);
      notify({
        tone: "success",
        text: r.updated
          ? `Paket v${r.packageVersion} tersimpan, ${r.assetsDownloaded} media diunduh (total ${r.assetsTotal}).`
          : `Paket sudah terbaru (v${r.packageVersion}). ${r.assetsDownloaded} media dilengkapi.`,
      });
      await loadLocal();
    } catch (err) {
      notify({ tone: "danger", text: errorText(err) });
    } finally {
      setBusyId(null);
    }
  };

  const localById = new Map(local.map((l) => [l.scheduleId, l]));

  return (
    <div className="stack">
      <section className="card">
        <div className="card-header">
          <h2>Jadwal dari server pusat</h2>
          <Button onClick={() => void loadRemote()} loading={loading}>Muat ulang</Button>
        </div>
        {remote === null ? (
          <p className="muted">{loading ? "Menghubungi server…" : "Tidak dapat memuat jadwal (periksa koneksi & pengaturan)."}</p>
        ) : !remote.length ? (
          <p className="muted">Belum ada jadwal terbit untuk lokasi ini.</p>
        ) : (
          <table className="table">
            <thead><tr><th>Ujian</th><th>Waktu</th><th>Paket di server</th><th>Di komputer ini</th><th /></tr></thead>
            <tbody>
              {remote.map((s) => {
                const l = localById.get(s.id);
                const upToDate = l && s.package && l.checksum === s.package.checksum && l.assetsMissing === 0;
                return (
                  <tr key={s.id}>
                    <td><strong>{s.exam.title}</strong><div className="muted small">{s.exam.code} · {s.name}</div></td>
                    <td className="small">{fmtDateTime(s.startAt)}<br />s.d. {fmtDateTime(s.endAt)}</td>
                    <td>{s.package ? `v${s.package.version}` : <span className="muted">belum terbit</span>}</td>
                    <td>
                      {!l ? <Badge>belum diunduh</Badge> : upToDate ? <Badge tone="success">terbaru v{l.packageVersion}</Badge> : <Badge tone="warning">perlu diperbarui</Badge>}
                    </td>
                    <td className="row-actions">
                      <Button variant={upToDate ? "secondary" : "primary"} disabled={!s.package} loading={busyId === s.id} onClick={() => void download(s.id)}>
                        {l ? "Perbarui" : "Unduh"}
                      </Button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </section>

      <section className="card">
        <h2>Paket tersimpan di komputer ini</h2>
        {!local.length ? (
          <p className="muted">Belum ada paket.</p>
        ) : (
          <table className="table">
            <thead><tr><th>Ujian</th><th>Waktu</th><th>Isi</th><th>Peserta ujian</th></tr></thead>
            <tbody>
              {local.map((l) => (
                <tr key={l.scheduleId}>
                  <td><strong>{l.examTitle}</strong><div className="muted small">{l.name} · paket v{l.packageVersion} · diunduh {fmtDateTime(l.downloadedAt)}</div></td>
                  <td className="small">{fmtDateTime(l.startAt)}<br />s.d. {fmtDateTime(l.endAt)}</td>
                  <td className="small">
                    {l.questionCount} soal · {l.participantCount} peserta · {l.assetCount} media
                    {l.assetsMissing ? <div><Badge tone="danger">{l.assetsMissing} media belum lengkap</Badge></div> : null}
                    {l.requiresToken ? <div className="muted">memakai token sesi</div> : null}
                  </td>
                  <td className="small">
                    {l.attempts.inProgress} berlangsung · {l.attempts.finished} selesai
                    {l.attempts.unsynced ? <div><Badge tone="warning">{l.attempts.unsynced} belum terkirim</Badge></div> : null}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>
    </div>
  );
}

function ResultsTab({ notify }: { notify: Notify }) {
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [local, setLocal] = useState<LocalSchedule[]>([]);
  const [scheduleId, setScheduleId] = useState("");
  const [attempts, setAttempts] = useState<AttemptRow[]>([]);
  const [syncing, setSyncing] = useState(false);

  const load = useCallback(async () => {
    setStatus(await ipc.syncStatus());
    const l = await ipc.localSchedules();
    setLocal(l);
    const sid = scheduleId || l[0]?.scheduleId || "";
    if (sid) {
      setScheduleId(sid);
      setAttempts(await ipc.listAttempts(sid));
    }
  }, [scheduleId]);

  useEffect(() => {
    load().catch((e) => notify({ tone: "danger", text: errorText(e) }));
    const t = setInterval(() => void load().catch(() => {}), 10_000);
    return () => clearInterval(t);
  }, [load, notify]);

  const syncNow = async () => {
    setSyncing(true);
    notify(null);
    try {
      const r = await ipc.syncNow();
      notify({
        tone: "success",
        text: r.attemptsSent ? `${r.attemptsSent} hasil ujian dan ${r.attachmentsUploaded} berkas terkirim.` : "Tidak ada hasil baru untuk dikirim.",
      });
      await load();
    } catch (err) {
      notify({ tone: "danger", text: errorText(err) });
    } finally {
      setSyncing(false);
    }
  };

  const reset = async (a: AttemptRow) => {
    if (!window.confirm(`Hapus data ujian ${a.participantNumber} di komputer ini? Hanya lakukan bila attempt peserta juga sudah dihapus di server pusat.`)) return;
    try {
      await ipc.resetAttempt(a.id);
      await load();
    } catch (err) {
      notify({ tone: "danger", text: errorText(err) });
    }
  };

  const c = status?.counts;
  return (
    <div className="stack">
      <section className="card">
        <div className="card-header">
          <h2>Pengiriman hasil</h2>
          <Button variant="primary" loading={syncing} onClick={() => void syncNow()}>Kirim hasil sekarang</Button>
        </div>
        {c ? (
          <div className="stats">
            <div className="stat"><span>Peserta ujian</span><strong>{c.total}</strong></div>
            <div className="stat"><span>Berlangsung</span><strong>{c.inProgress}</strong></div>
            <div className="stat"><span>Selesai</span><strong>{c.finished}</strong></div>
            <div className={`stat${c.unsynced ? " stat-warning" : ""}`}><span>Belum terkirim</span><strong>{c.unsynced}</strong></div>
            <div className={`stat${c.syncErrors ? " stat-danger" : ""}`}><span>Ditolak server</span><strong>{c.syncErrors}</strong></div>
          </div>
        ) : null}
        <p className="muted small">
          Terakhir berhasil: {fmtDateTime(status?.lastSyncAt)} · Kirim otomatis: {status?.autoSync ? "aktif" : "nonaktif"}
        </p>
        {status?.lastError ? <Alert>{status.lastError}</Alert> : null}
      </section>

      <section className="card">
        <div className="card-header">
          <h2>Peserta di komputer ini</h2>
          <select className="input input-inline" value={scheduleId} onChange={(e) => { setScheduleId(e.target.value); }}>
            {local.map((l) => <option key={l.scheduleId} value={l.scheduleId}>{l.examTitle} · {l.name}</option>)}
          </select>
        </div>
        {!attempts.length ? (
          <p className="muted">Belum ada peserta yang ujian di komputer ini.</p>
        ) : (
          <table className="table">
            <thead><tr><th>Peserta</th><th>Status</th><th>Mulai</th><th>Selesai</th><th>Dijawab</th><th>Pelanggaran</th><th>Terkirim</th><th /></tr></thead>
            <tbody>
              {attempts.map((a) => (
                <tr key={a.id}>
                  <td><span className="mono">{a.participantNumber}</span> {a.participantName}</td>
                  <td><StatusBadge status={a.status} /></td>
                  <td className="small">{fmtDateTime(a.startedAt)}</td>
                  <td className="small">{fmtDateTime(a.finishedAt)}</td>
                  <td>{a.answered}</td>
                  <td>{a.violationCount ? <Badge tone="warning">{a.violationCount}</Badge> : 0}</td>
                  <td>{a.syncError ? <Badge tone="danger">ditolak: {a.syncError}</Badge> : a.synced ? <Badge tone="success">ya</Badge> : <Badge tone="warning">belum</Badge>}</td>
                  <td><Button variant="ghost" onClick={() => void reset(a)}>Reset</Button></td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>

      <section className="card">
        <h2>Riwayat pengiriman</h2>
        {!status?.batches.length ? (
          <p className="muted">Belum ada.</p>
        ) : (
          <table className="table">
            <thead><tr><th>Waktu</th><th>Jumlah</th><th>Status di server</th><th>Ditolak</th></tr></thead>
            <tbody>
              {status.batches.map((b) => {
                const rejected = b.response?.attempts?.filter((a) => !a.accepted) ?? [];
                return (
                  <tr key={b.id}>
                    <td className="small">{fmtDateTime(b.createdAt)}<div className="muted mono">{b.id.slice(0, 8)}</div></td>
                    <td>{b.attemptCount}</td>
                    <td><StatusBadge status={b.status} /></td>
                    <td className="small">{rejected.length ? rejected.map((r) => <div key={r.attemptId}>{r.reason}</div>) : "-"}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </section>
    </div>
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
        text: `Terhubung sebagai ${r.site.code} (${r.site.name}).${Math.abs(skew) > 120 ? ` Jam komputer ini selisih ${skew} detik dari server, sesuaikan jam komputer!` : ""}`,
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
          Tes koneksi memakai alamat yang sudah disimpan: <span className="mono">{config.serverUrl ?? "-"}</span>.
          Tekan Simpan dulu bila alamat di bawah diubah.
        </p>
        <ConfigForm config={config} onSaved={onConfigChanged} />
      </section>
      <section className="card">
        <h2>Tentang</h2>
        <dl className="kv">
          <dt>Versi aplikasi</dt><dd>{info?.version ?? "-"}</dd>
          <dt>ID perangkat</dt><dd className="mono">{config.deviceId}</dd>
          <dt>Folder data</dt><dd className="mono small">{info?.dataDir ?? "-"}</dd>
        </dl>
      </section>
    </div>
  );
}
