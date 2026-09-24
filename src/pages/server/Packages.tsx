import { useCallback, useEffect, useState } from "react";
import { Alert, Badge, Button, StatusBadge } from "../../components/ui";
import { fmtDateTime } from "../../lib/format";
import { errorText, ipc, type LocalSchedule, type RemoteSchedule, type SyncStatus } from "../../lib/ipc";
import type { Notify } from "./Dashboard";

export function PackagesTab({ notify }: { notify: Notify }) {
  const [remote, setRemote] = useState<RemoteSchedule[] | null>(null);
  const [local, setLocal] = useState<LocalSchedule[]>([]);
  const [loading, setLoading] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);

  const loadLocal = useCallback(() => ipc.localSchedules().then(setLocal), []);
  const loadRemote = useCallback(async () => {
    setLoading(true);
    try {
      setRemote(await ipc.remoteSchedules());
      await loadLocal();
    } catch (err) {
      notify({ tone: "danger", text: errorText(err) });
    } finally {
      setLoading(false);
    }
  }, [notify, loadLocal]);

  useEffect(() => {
    loadLocal().catch((e) => notify({ tone: "danger", text: errorText(e) }));
    void loadRemote();
  }, [loadLocal, loadRemote, notify]);

  const download = async (id: string) => {
    setBusyId(id);
    notify(null);
    try {
      const r = await ipc.downloadSchedule(id);
      notify({
        tone: "success",
        text: r.updated
          ? `Paket v${r.packageVersion} tersimpan, ${r.assetsDownloaded} media diunduh (total ${r.assetsTotal}). PC peserta langsung bisa memakainya.`
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
          <p className="muted">{loading ? "Menghubungi server pusat…" : "Tidak dapat memuat jadwal (periksa koneksi internet & pengaturan)."}</p>
        ) : !remote.length ? (
          <p className="muted">Belum ada jadwal terbit untuk lokasi ini.</p>
        ) : (
          <table className="table">
            <thead><tr><th>Ujian</th><th>Waktu</th><th>Paket di pusat</th><th>Di server lokal</th><th /></tr></thead>
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
        <h2>Paket tersimpan di server lokal</h2>
        {!local.length ? (
          <p className="muted">Belum ada paket.</p>
        ) : (
          <table className="table">
            <thead><tr><th>Ujian</th><th>Waktu</th><th>Isi</th><th>Token sesi</th><th>Peserta</th></tr></thead>
            <tbody>
              {local.map((l) => (
                <tr key={l.scheduleId}>
                  <td><strong>{l.examTitle}</strong><div className="muted small">{l.name} · paket v{l.packageVersion} · diunduh {fmtDateTime(l.downloadedAt)}</div></td>
                  <td className="small">{fmtDateTime(l.startAt)}<br />s.d. {fmtDateTime(l.endAt)}</td>
                  <td className="small">
                    {l.questionCount} soal · {l.participantCount} peserta · {l.assetCount} media
                    {l.assetsMissing ? <div><Badge tone="danger">{l.assetsMissing} media belum lengkap</Badge></div> : null}
                  </td>
                  <td className="mono">{l.requiresToken ? l.accessToken ?? "?" : <span className="muted">tanpa token</span>}</td>
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

export function ResultsTab({ notify }: { notify: Notify }) {
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [syncing, setSyncing] = useState(false);

  const load = useCallback(async () => setStatus(await ipc.syncStatus()), []);

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
        text: r.attemptsSent ? `${r.attemptsSent} hasil ujian dan ${r.attachmentsUploaded} berkas terkirim ke server pusat.` : "Tidak ada hasil baru untuk dikirim.",
      });
      await load();
    } catch (err) {
      notify({ tone: "danger", text: errorText(err) });
    } finally {
      setSyncing(false);
    }
  };

  const c = status?.counts;
  return (
    <div className="stack">
      <section className="card">
        <div className="card-header">
          <h2>Pengiriman hasil ke server pusat</h2>
          <Button variant="primary" loading={syncing} onClick={() => void syncNow()}>Kirim hasil sekarang</Button>
        </div>
        {c ? (
          <div className="stats">
            <div className="stat"><span>Peserta ujian</span><strong>{c.total}</strong></div>
            <div className="stat"><span>Berlangsung</span><strong>{c.inProgress}</strong></div>
            <div className="stat"><span>Selesai</span><strong>{c.finished}</strong></div>
            <div className={`stat${c.unsynced ? " stat-warning" : ""}`}><span>Belum terkirim</span><strong>{c.unsynced}</strong></div>
            <div className={`stat${c.syncErrors ? " stat-danger" : ""}`}><span>Ditolak pusat</span><strong>{c.syncErrors}</strong></div>
          </div>
        ) : null}
        <p className="muted small">
          Terakhir berhasil: {fmtDateTime(status?.lastSyncAt)} · Kirim otomatis: {status?.autoSync ? "aktif" : "nonaktif"}
          {status?.proctorLogPending ? ` · ${status.proctorLogPending} log proktor belum terkirim` : ""}
        </p>
        <p className="muted small">Pastikan <b>Belum terkirim</b> bernilai 0 sebelum server lokal dimatikan setelah ujian.</p>
        {status?.lastError ? <Alert>{status.lastError}</Alert> : null}
      </section>

      <section className="card">
        <h2>Riwayat pengiriman</h2>
        {!status?.batches.length ? (
          <p className="muted">Belum ada.</p>
        ) : (
          <table className="table">
            <thead><tr><th>Waktu</th><th>Jumlah</th><th>Status di pusat</th><th>Ditolak</th></tr></thead>
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
