import { useCallback, useEffect, useState } from "react";
import { Alert, Badge, Button } from "../../components/ui";
import { fmtDateTime } from "../../lib/format";
import { errorText, ipc, type Device, type LanInfo } from "../../lib/ipc";
import type { Notify } from "./Dashboard";

const ONLINE_MS = 30_000;

export function DevicesTab({ notify }: { notify: Notify }) {
  const [lan, setLan] = useState<LanInfo | null>(null);
  const [devices, setDevices] = useState<Device[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const load = useCallback(async () => {
    const [l, d] = await Promise.all([ipc.lanInfo(), ipc.listDevices()]);
    setLan(l);
    setDevices(d);
  }, []);

  useEffect(() => {
    load().catch((e) => notify({ tone: "danger", text: errorText(e) }));
    const t = setInterval(() => void load().catch(() => {}), 3_000);
    return () => clearInterval(t);
  }, [load, notify]);

  const act = async (d: Device, fn: () => Promise<void>, text: string) => {
    setBusy(d.id);
    try {
      await fn();
      notify({ tone: "success", text });
      await load();
    } catch (err) {
      notify({ tone: "danger", text: errorText(err) });
    } finally {
      setBusy(null);
    }
  };

  const now = Date.now();
  const pending = devices?.filter((d) => d.status === "pending") ?? [];

  return (
    <div className="stack">
      <section className="card">
        <h2>Alamat server lokal</h2>
        {lan?.running ? (
          <>
            <p>Di setiap PC peserta, isi alamat server lokal dengan salah satu alamat berikut (yang satu jaringan dengan PC peserta):</p>
            <div className="address-list">
              {lan.addresses.length ? lan.addresses.map((a) => <code key={a} className="address">{a}</code>) : <span className="muted">Tidak ada alamat jaringan. Periksa kabel/WiFi.</span>}
            </div>
            <p className="muted small">
              {lan.devicesOnline} PC peserta terhubung. Bila PC peserta tidak bisa terhubung, izinkan aplikasi ini di Windows Firewall
              (jaringan Private) untuk port {lan.port}.
            </p>
          </>
        ) : (
          <Alert>{lan?.error ?? "Layanan LAN belum berjalan."} Ganti port di tab Pengaturan bila port dipakai aplikasi lain.</Alert>
        )}
      </section>

      <section className="card">
        <div className="card-header">
          <h2>PC peserta</h2>
          {pending.length ? (
            <Button
              variant="primary"
              onClick={() =>
                void (async () => {
                  for (const d of pending) await ipc.setDeviceStatus(d.id, true);
                  notify({ tone: "success", text: `${pending.length} PC disetujui.` });
                  await load();
                })().catch((e) => notify({ tone: "danger", text: errorText(e) }))
              }
            >
              Setujui semua ({pending.length})
            </Button>
          ) : null}
        </div>
        <p className="muted small">
          PC baru muncul setelah alamat server lokal diisi di PC tersebut. Cocokkan kode di layar PC peserta sebelum menyetujui.
        </p>
        {!devices ? <p className="muted">Memuat…</p> : !devices.length ? <p className="muted">Belum ada PC peserta yang mendaftar.</p> : (
          <table className="table">
            <thead><tr><th>Nama</th><th>Kode</th><th>Status</th><th>IP</th><th>Terakhir terhubung</th><th /></tr></thead>
            <tbody>
              {devices.map((d) => {
                const on = !!d.lastSeenAt && now - new Date(d.lastSeenAt).getTime() < ONLINE_MS;
                return (
                  <tr key={d.id} className={d.status === "pending" ? "row-highlight" : undefined}>
                    <td><span className={`dot ${on ? "dot-on" : "dot-off"}`} /> <strong>{d.name}</strong><div className="muted small mono">{d.id.slice(0, 8)}</div></td>
                    <td className="mono">{d.pairingCode}</td>
                    <td>
                      {d.status === "approved" ? <Badge tone="success">disetujui</Badge> : d.status === "pending" ? <Badge tone="warning">menunggu</Badge> : <Badge tone="danger">dicabut</Badge>}
                      {d.approvedBy ? <div className="muted small">oleh {d.approvedBy}</div> : null}
                    </td>
                    <td className="mono small">{d.ip ?? "-"}</td>
                    <td className="small">{fmtDateTime(d.lastSeenAt)}</td>
                    <td className="row-actions">
                      {d.status !== "approved" ? (
                        <Button variant="primary" loading={busy === d.id} onClick={() => void act(d, () => ipc.setDeviceStatus(d.id, true), `${d.name} disetujui.`)}>Setujui</Button>
                      ) : (
                        <Button variant="ghost" loading={busy === d.id} onClick={() => void act(d, () => ipc.setDeviceStatus(d.id, false), `${d.name} dicabut.`)}>Cabut</Button>
                      )}
                      <Button
                        variant="ghost"
                        onClick={() => {
                          if (window.confirm(`Hapus ${d.name} dari daftar? PC tersebut harus mendaftar ulang.`))
                            void act(d, () => ipc.deleteDevice(d.id), `${d.name} dihapus.`);
                        }}
                      >
                        Hapus
                      </Button>
                    </td>
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
