import { useState, type FormEvent } from "react";
import { Alert, Button, Field, Input } from "../components/ui";
import { errorText, ipc, type ConfigView } from "../lib/ipc";

/** Form konfigurasi koneksi ke server pusat. Dipakai saat pertama kali dan di menu operator. */
export function ConfigForm({ config, onSaved, submitLabel = "Simpan" }: { config: ConfigView | null; onSaved: (c: ConfigView) => void; submitLabel?: string }) {
  const [form, setForm] = useState({
    serverUrl: config?.serverUrl ?? "",
    siteCode: config?.siteCode ?? "",
    secret: "",
    pin: "",
    pin2: "",
    deviceName: config?.deviceName ?? "",
    autoSync: config?.autoSync ?? true,
  });
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    setSaved(false);
    if (form.pin !== form.pin2) {
      setError("Konfirmasi PIN tidak sama");
      return;
    }
    setBusy(true);
    try {
      const cfg = await ipc.saveConfig({
        serverUrl: form.serverUrl,
        siteCode: form.siteCode,
        secret: form.secret || null,
        operatorPin: form.pin || null,
        deviceName: form.deviceName || null,
        autoSync: form.autoSync,
      });
      setForm((f) => ({ ...f, secret: "", pin: "", pin2: "" }));
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
      <Field label="Alamat server pusat" hint="mis. https://cbt.sekolah.sch.id atau http://192.168.1.10:8080">
        <Input value={form.serverUrl} onChange={(e) => setForm({ ...form, serverUrl: e.target.value })} placeholder="https://" required />
      </Field>
      <div className="grid-2">
        <Field label="Kode lokasi (titik ujian)">
          <Input value={form.siteCode} onChange={(e) => setForm({ ...form, siteCode: e.target.value.toUpperCase() })} required />
        </Field>
        <Field label="Secret lokasi" hint={config?.hasSecret ? "Kosongkan bila tidak diubah" : "Dari panel admin, menu Titik Ujian"}>
          <Input type="password" value={form.secret} onChange={(e) => setForm({ ...form, secret: e.target.value })} required={!config?.hasSecret} />
        </Field>
        <Field label={config?.hasPin ? "PIN operator baru" : "PIN operator"} hint={config?.hasPin ? "Kosongkan bila tidak diubah" : "Minimal 4 karakter, untuk membuka menu operator"}>
          <Input type="password" inputMode="numeric" value={form.pin} onChange={(e) => setForm({ ...form, pin: e.target.value })} required={!config?.hasPin} />
        </Field>
        <Field label="Ulangi PIN">
          <Input type="password" inputMode="numeric" value={form.pin2} onChange={(e) => setForm({ ...form, pin2: e.target.value })} required={!config?.hasPin || !!form.pin} />
        </Field>
        <Field label="Nama komputer (opsional)" hint="Membantu melacak hasil per komputer, mis. LAB1-PC07">
          <Input value={form.deviceName} onChange={(e) => setForm({ ...form, deviceName: e.target.value })} />
        </Field>
        <label className="checkbox">
          <input type="checkbox" checked={form.autoSync} onChange={(e) => setForm({ ...form, autoSync: e.target.checked })} />
          <span>Kirim hasil otomatis saat online (tiap 1 menit)</span>
        </label>
      </div>
      <Alert>{error}</Alert>
      {saved ? <Alert tone="success">Konfigurasi tersimpan.</Alert> : null}
      <div className="actions">
        <Button type="submit" variant="primary" loading={busy}>{submitLabel}</Button>
      </div>
    </form>
  );
}

export function SetupPage({ config, onSaved }: { config: ConfigView | null; onSaved: () => void }) {
  return (
    <div className="center-screen">
      <div className="card setup-card">
        <div className="brand"><span className="brand-logo">CBT</span> Pengaturan awal titik ujian</div>
        <p className="muted">
          Hubungkan komputer ini ke server pusat. Kode lokasi dan secret dibuat admin di panel server pusat (menu <b>Titik Ujian</b>).
        </p>
        <ConfigForm config={config} onSaved={onSaved} submitLabel="Simpan & lanjut" />
      </div>
    </div>
  );
}
