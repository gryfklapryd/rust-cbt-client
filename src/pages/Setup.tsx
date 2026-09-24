import { useState, type FormEvent } from "react";
import { Alert, Button, Field, Input } from "../components/ui";
import { errorText, ipc, type AppMode, type ConfigView } from "../lib/ipc";

/** Form koneksi server lokal ke server pusat. Dipakai saat pertama kali dan di menu Pengaturan. */
export function ServerConfigForm({ config, onSaved, submitLabel = "Simpan" }: { config: ConfigView | null; onSaved: (c: ConfigView) => void; submitLabel?: string }) {
  const [form, setForm] = useState({
    serverUrl: config?.serverUrl ?? "",
    siteCode: config?.siteCode ?? "",
    secret: "",
    deviceName: config?.deviceName ?? "",
    lanPort: String(config?.lanPort ?? 8787),
    autoSync: config?.autoSync ?? true,
  });
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    setSaved(false);
    setBusy(true);
    try {
      const cfg = await ipc.saveServerConfig({
        serverUrl: form.serverUrl,
        siteCode: form.siteCode,
        secret: form.secret || null,
        deviceName: form.deviceName || null,
        lanPort: Number(form.lanPort) || null,
        autoSync: form.autoSync,
      });
      setForm((f) => ({ ...f, secret: "" }));
      setSaved(true);
      onSaved(cfg);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit} className="stack">
      <Field label="Alamat server pusat" hint="mis. https://cbt.dinas.go.id atau http://192.168.1.5:8080">
        <Input value={form.serverUrl} onChange={(e) => setForm({ ...form, serverUrl: e.target.value })} placeholder="https://" required />
      </Field>
      <div className="grid-2">
        <Field label="Kode lokasi (titik ujian)">
          <Input value={form.siteCode} onChange={(e) => setForm({ ...form, siteCode: e.target.value.toUpperCase() })} required />
        </Field>
        <Field label="Secret lokasi" hint={config?.hasSecret ? "Kosongkan bila tidak diubah" : "Dari panel admin pusat, menu Titik Ujian"}>
          <Input type="password" value={form.secret} onChange={(e) => setForm({ ...form, secret: e.target.value })} required={!config?.hasSecret} />
        </Field>
        <Field label="Nama server (opsional)" hint="mis. SERVER-LAB1">
          <Input value={form.deviceName} onChange={(e) => setForm({ ...form, deviceName: e.target.value })} />
        </Field>
        <Field label="Port LAN" hint="PC peserta terhubung ke alamat IP komputer ini dengan port ini">
          <Input type="number" min={1} max={65535} value={form.lanPort} onChange={(e) => setForm({ ...form, lanPort: e.target.value })} required />
        </Field>
      </div>
      <label className="checkbox">
        <input type="checkbox" checked={form.autoSync} onChange={(e) => setForm({ ...form, autoSync: e.target.checked })} />
        <span>Kirim hasil ke server pusat otomatis saat online (tiap 1 menit)</span>
      </label>
      <Alert>{error}</Alert>
      {saved ? <Alert tone="success">Pengaturan tersimpan.</Alert> : null}
      <div className="actions">
        <Button type="submit" variant="primary" loading={busy}>{submitLabel}</Button>
      </div>
    </form>
  );
}

/** Form alamat server lokal untuk PC peserta. */
export function ParticipantConfigForm({ config, onSaved, submitLabel = "Simpan" }: { config: ConfigView | null; onSaved: (c: ConfigView) => void; submitLabel?: string }) {
  const [form, setForm] = useState({
    lanUrl: config?.lanUrl?.replace(/^http:\/\//, "") ?? "",
    deviceName: config?.deviceName ?? "",
  });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      onSaved(await ipc.saveParticipantConfig({ lanUrl: form.lanUrl, deviceName: form.deviceName || null }));
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit} className="stack">
      <Field label="Alamat IP server lokal" hint="Tertera di layar server lokal (menu Perangkat), mis. 192.168.1.10 atau 192.168.1.10:8787">
        <Input value={form.lanUrl} onChange={(e) => setForm({ ...form, lanUrl: e.target.value })} placeholder="192.168.1.10" required autoFocus />
      </Field>
      <Field label="Nama komputer" hint="Tampil di dasbor proktor, mis. LAB1-PC07">
        <Input value={form.deviceName} onChange={(e) => setForm({ ...form, deviceName: e.target.value })} required />
      </Field>
      <Alert>{error}</Alert>
      <div className="actions">
        <Button type="submit" variant="primary" loading={busy}>{submitLabel}</Button>
      </div>
    </form>
  );
}

export function SetupPage({ config, onSaved }: { config: ConfigView | null; onSaved: () => void }) {
  const [mode, setMode] = useState<AppMode | null>(config?.mode ?? null);

  return (
    <div className="center-screen">
      <div className="card setup-card">
        <div className="brand"><span className="brand-logo">CBT</span> Pengaturan awal</div>
        {!mode ? (
          <>
            <p className="muted">Pilih peran komputer ini di titik ujian. Pilihan ini tidak bisa diubah kemudian.</p>
            <div className="mode-grid">
              <button type="button" className="mode-card" onClick={() => setMode("server")}>
                <strong>Server lokal</strong>
                <span className="muted small">
                  Satu komputer per titik ujian, dipegang proktor. Terhubung ke server pusat, mengunduh paket, melayani PC peserta
                  lewat jaringan lokal, dan mengirim hasil.
                </span>
              </button>
              <button type="button" className="mode-card" onClick={() => setMode("participant")}>
                <strong>PC peserta</strong>
                <span className="muted small">
                  Komputer tempat peserta mengerjakan ujian. Cukup diisi alamat IP server lokal, lalu disetujui proktor.
                </span>
              </button>
            </div>
          </>
        ) : mode === "server" ? (
          <>
            <p className="muted">
              Hubungkan server lokal ke server pusat. Kode lokasi dan secret dibuat admin di panel pusat (menu <b>Titik Ujian</b>).
              Proktor login memakai akun pusat yang ditugaskan ke lokasi ini.
            </p>
            <ServerConfigForm config={config} onSaved={onSaved} submitLabel="Simpan & lanjut" />
          </>
        ) : (
          <>
            <p className="muted">Hubungkan komputer ini ke server lokal di titik ujian.</p>
            <ParticipantConfigForm config={config} onSaved={onSaved} submitLabel="Simpan & daftarkan" />
          </>
        )}
        {mode && !config?.mode ? (
          <button type="button" className="link-btn" onClick={() => setMode(null)}>Kembali pilih peran</button>
        ) : null}
      </div>
    </div>
  );
}
