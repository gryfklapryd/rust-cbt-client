import { useEffect, useState, type FormEvent } from "react";
import { Alert, Button, Field, Input } from "../components/ui";
import { errorText, ipc, type ConfigView, type LanInfo, type Proctor } from "../lib/ipc";

/** Login proktor dengan akun pusat. Di server lokal diverifikasi lokal, di PC peserta lewat server lokal. */
export function ProctorLoginForm({ onLoggedIn, formId }: { onLoggedIn: (p: Proctor) => void; formId?: string }) {
  const [form, setForm] = useState({ username: "", password: "" });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const p = await ipc.proctorLogin(form.username, form.password);
      setForm({ username: "", password: "" });
      onLoggedIn(p);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form id={formId} onSubmit={submit}>
      <Field label="Username proktor">
        <Input autoFocus autoComplete="off" value={form.username} onChange={(e) => setForm({ ...form, username: e.target.value })} required />
      </Field>
      <Field label="Password">
        <Input type="password" value={form.password} onChange={(e) => setForm({ ...form, password: e.target.value })} required />
      </Field>
      <Alert>{error}</Alert>
      {formId ? null : <Button type="submit" variant="primary" className="btn-lg" loading={busy}>Masuk</Button>}
    </form>
  );
}

/** Layar awal server lokal: login proktor. */
export function ServerLoginPage({ config, onLoggedIn }: { config: ConfigView; onLoggedIn: (p: Proctor) => void }) {
  const [count, setCount] = useState<number | null>(null);
  const [lan, setLan] = useState<LanInfo | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [message, setMessage] = useState<{ tone: "success" | "danger"; text: string } | null>(null);

  useEffect(() => {
    ipc.proctorCount().then(setCount).catch(() => setCount(0));
    const load = () => ipc.lanInfo().then(setLan).catch(() => {});
    void load();
    const t = setInterval(load, 10_000);
    return () => clearInterval(t);
  }, []);

  const syncProctors = async () => {
    setSyncing(true);
    setMessage(null);
    try {
      const r = await ipc.syncProctors();
      setCount(r.count);
      setMessage(
        r.count
          ? { tone: "success", text: `${r.count} akun proktor diperbarui dari server pusat.` }
          : { tone: "danger", text: `Belum ada proktor yang ditugaskan ke lokasi ${config.siteCode} di server pusat.` },
      );
    } catch (err) {
      setMessage({ tone: "danger", text: errorText(err) });
    } finally {
      setSyncing(false);
    }
  };

  return (
    <div className="center-screen">
      <div className="card login-card">
        <div className="brand"><span className="brand-logo">CBT</span> Server lokal · {config.siteCode}</div>
        <h1>Login proktor</h1>
        {count === 0 ? (
          <Alert tone="warning">
            Belum ada akun proktor di server lokal ini. Minta admin pusat menugaskan proktor ke lokasi {config.siteCode}
            (menu Titik Ujian → Proktor), lalu tekan tombol di bawah saat komputer ini terhubung ke internet.
          </Alert>
        ) : null}
        <ProctorLoginForm onLoggedIn={onLoggedIn} />
        {message ? <Alert tone={message.tone}>{message.text}</Alert> : null}
        <div className="login-extra">
          <Button variant="ghost" loading={syncing} onClick={() => void syncProctors()}>Perbarui akun proktor dari server pusat</Button>
        </div>
        {lan ? (
          <p className="muted small">
            {lan.running ? (
              <>Alamat untuk PC peserta: <span className="mono">{lan.addresses.join(", ") || `port ${lan.port}`}</span> · {lan.devicesOnline} PC terhubung</>
            ) : (
              <span className="text-danger">{lan.error ?? "Layanan LAN belum berjalan"}</span>
            )}
          </p>
        ) : null}
      </div>
    </div>
  );
}
