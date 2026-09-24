import { useCallback, useEffect, useMemo, useState, type FormEvent } from "react";
import { Alert, Button, Field, Input, Modal } from "../components/ui";
import { errorText, ipc, type ConfigView, type ExamSchedule, type ExamSession, type LinkStatus, type Proctor } from "../lib/ipc";
import { fmtDateTime, fmtTime } from "../lib/format";
import { ProctorLoginForm } from "./ProctorLogin";

/** Jadwal yang bisa dipilih peserta: sedang berlangsung atau mulai dalam 30 menit. */
function isOpen(s: ExamSchedule, now: number) {
  return now >= new Date(s.startAt).getTime() - 30 * 60_000 && now < new Date(s.endAt).getTime();
}

/** Layar login peserta di PC peserta. */
export function HomePage({ config, onLogin, onProctor }: { config: ConfigView; onLogin: (s: ExamSession) => void; onProctor: (p: Proctor) => void }) {
  const [schedules, setSchedules] = useState<ExamSchedule[] | null>(null);
  const [link, setLink] = useState<LinkStatus | null>(null);
  const [now, setNow] = useState(Date.now());
  const [scheduleId, setScheduleId] = useState("");
  const [form, setForm] = useState({ number: "", password: "", token: "" });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [proctorOpen, setProctorOpen] = useState(false);

  const load = useCallback(async () => {
    try {
      setSchedules(await ipc.examSchedules());
    } catch {
      // Status koneksi ditampilkan terpisah.
    }
    setLink(await ipc.linkStatus().catch((e) => ({ connected: false, pending: 0, error: errorText(e) })));
  }, []);

  useEffect(() => {
    void load();
    const t = setInterval(() => {
      setNow(Date.now());
      void load();
    }, 15_000);
    return () => clearInterval(t);
  }, [load]);

  const open = useMemo(() => (schedules ?? []).filter((s) => isOpen(s, now)), [schedules, now]);
  const selected = open.find((s) => s.scheduleId === scheduleId) ?? (open.length === 1 ? open[0] : undefined);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      const session = await ipc.login({
        scheduleId: selected.scheduleId,
        number: form.number,
        password: form.password,
        token: selected.requiresToken ? form.token : undefined,
      });
      setForm({ number: "", password: "", token: "" });
      onLogin(session);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="home">
      <header className="home-header">
        <div className="brand"><span className="brand-logo">CBT</span> {config.deviceName ?? "PC peserta"}</div>
        <div className="clock">{new Intl.DateTimeFormat("id-ID", { dateStyle: "full", timeStyle: "short" }).format(now)}</div>
      </header>

      <main className="center-screen">
        <form className="card login-card" onSubmit={submit}>
          <h1>Masuk ujian</h1>
          {link && !link.connected ? (
            <Alert>Tidak terhubung ke server lokal. Hubungi pengawas.{link.error ? `\n(${link.error})` : ""}</Alert>
          ) : null}
          {schedules === null ? (
            <p className="muted">Memuat jadwal…</p>
          ) : !open.length ? (
            <Alert tone="info">
              Tidak ada ujian yang dibuka saat ini.
              {schedules.length ? (
                <> Jadwal berikutnya: {schedules.filter((s) => new Date(s.endAt).getTime() > now).map((s) => `${s.examTitle} (${fmtDateTime(s.startAt)})`)[0] ?? "-"}</>
              ) : (
                <> Proktor perlu mengunduh paket ujian di server lokal terlebih dahulu.</>
              )}
            </Alert>
          ) : (
            <>
              {open.length > 1 ? (
                <Field label="Ujian">
                  <select className="input" value={selected?.scheduleId ?? ""} onChange={(e) => setScheduleId(e.target.value)} required>
                    <option value="">Pilih ujian…</option>
                    {open.map((s) => (
                      <option key={s.scheduleId} value={s.scheduleId}>
                        {s.examTitle} · {s.name} ({fmtTime(s.startAt)} - {fmtTime(s.endAt)})
                      </option>
                    ))}
                  </select>
                </Field>
              ) : selected ? (
                <div className="exam-pill">
                  <strong>{selected.examTitle}</strong>
                  <span className="muted">{selected.name} · {fmtTime(selected.startAt)} - {fmtTime(selected.endAt)} · {selected.durationMinutes} menit</span>
                </div>
              ) : null}
              {selected && !selected.ready ? <Alert tone="warning">Media ujian di server lokal belum lengkap. Hubungi pengawas.</Alert> : null}
              <Field label="Nomor peserta">
                <Input autoFocus autoComplete="off" value={form.number} onChange={(e) => setForm({ ...form, number: e.target.value })} required />
              </Field>
              <Field label="Password">
                <Input type="password" autoComplete="off" value={form.password} onChange={(e) => setForm({ ...form, password: e.target.value })} required />
              </Field>
              {selected?.requiresToken ? (
                <Field label="Token sesi" hint="Dibacakan pengawas saat ujian dimulai">
                  <Input className="token-input" autoComplete="off" value={form.token} onChange={(e) => setForm({ ...form, token: e.target.value.toUpperCase() })} required />
                </Field>
              ) : null}
              <Alert>{error}</Alert>
              <Button type="submit" variant="primary" className="btn-lg" loading={busy} disabled={!selected}>Masuk</Button>
            </>
          )}
        </form>
      </main>

      <footer className="home-footer">
        <span className="muted small">
          <span className={`dot ${link?.connected ? "dot-on" : "dot-off"}`} /> {link?.connected ? "Terhubung ke server lokal" : "Terputus"}
          {link?.pending ? ` · ${link.pending} data menunggu dikirim` : ""}
        </span>
        <button type="button" className="link-btn" onClick={() => setProctorOpen(true)}>Proktor</button>
      </footer>

      <Modal
        open={proctorOpen}
        title="Menu proktor"
        onClose={() => setProctorOpen(false)}
        footer={<><Button onClick={() => setProctorOpen(false)}>Batal</Button><Button variant="primary" type="submit" form="proctor-form">Masuk</Button></>}
      >
        <ProctorLoginForm
          formId="proctor-form"
          onLoggedIn={(p) => {
            setProctorOpen(false);
            onProctor(p);
          }}
        />
      </Modal>
    </div>
  );
}
