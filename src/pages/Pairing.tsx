import { useEffect, useRef, useState } from "react";
import { Alert, Button } from "../components/ui";
import { errorText, ipc, type ConfigView } from "../lib/ipc";
import { ParticipantConfigForm } from "./Setup";

const POLL_MS = 3_000;

/** PC peserta: mendaftar ke server lokal dan menunggu persetujuan proktor. */
export function PairingPage({ config, onApproved, onConfigChanged }: { config: ConfigView; onApproved: () => void; onConfigChanged: (c: ConfigView) => void }) {
  const [status, setStatus] = useState<{ status: string; pairingCode: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const done = useRef(false);

  useEffect(() => {
    if (editing) return;
    let alive = true;
    const poll = async () => {
      try {
        const r = await ipc.pairDevice();
        if (!alive) return;
        setError(null);
        setStatus(r);
        if (r.status === "approved" && !done.current) {
          done.current = true;
          onApproved();
        }
      } catch (err) {
        if (alive) setError(errorText(err));
      }
    };
    void poll();
    const t = setInterval(() => void poll(), POLL_MS);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, [editing, onApproved]);

  return (
    <div className="center-screen">
      <div className="card login-card">
        <div className="brand"><span className="brand-logo">CBT</span> PC peserta · {config.deviceName ?? config.deviceId.slice(0, 8)}</div>
        {editing ? (
          <>
            <h1>Alamat server lokal</h1>
            <ParticipantConfigForm
              config={config}
              onSaved={(c) => {
                setEditing(false);
                setStatus(null);
                onConfigChanged(c);
              }}
            />
            <button type="button" className="link-btn" onClick={() => setEditing(false)}>Batal</button>
          </>
        ) : (
          <>
            <h1>Pendaftaran komputer</h1>
            {error ? (
              <Alert>
                Tidak dapat terhubung ke server lokal di <span className="mono">{config.lanUrl}</span>. Pastikan server lokal menyala dan
                alamatnya benar.{"\n"}({error})
              </Alert>
            ) : status?.status === "pending" ? (
              <>
                <p>Minta proktor menyetujui komputer ini di server lokal (menu <b>PC peserta</b>). Kode komputer ini:</p>
                <div className="pair-code mono">{status.pairingCode}</div>
                <p className="muted small">Halaman ini lanjut otomatis setelah disetujui.</p>
              </>
            ) : (
              <p className="muted">Menghubungi server lokal…</p>
            )}
            <p className="muted small">Server lokal: <span className="mono">{config.lanUrl}</span></p>
            <Button variant="ghost" onClick={() => setEditing(true)}>Ubah alamat server lokal</Button>
          </>
        )}
      </div>
    </div>
  );
}
