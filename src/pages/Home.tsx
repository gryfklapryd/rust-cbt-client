import { useEffect, useMemo, useState, type FormEvent } from "react";
import { Alert, Button, Field, Input, Modal } from "../components/ui";
import { errorText, ipc, type ConfigView, type ExamSession, type LocalSchedule } from "../lib/ipc";
import { fmtDateTime, fmtTime } from "../lib/format";

/** Jadwal yang bisa dipilih peserta: sedang berlangsung atau mulai dalam 30 menit. */
function isOpen(s: LocalSchedule, now: number) {
  return now >= new Date(s.startAt).getTime() - 30 * 60_000 && now < new Date(s.endAt).getTime();
}

export function HomePage({ config, onLogin, onOperator }: { config: ConfigView; onLogin: (s: ExamSession) => void; onOperator: () => void }) {
  const [schedules, setSchedules] = useState<LocalSchedule[] | null>(null);
  const [now, setNow] = useState(Date.now());
  const [scheduleId, setScheduleId] = useState("");
  const [form, setForm] = useState({ number: "", password: "", token: "" });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [pinOpen, setPinOpen] = useState(false);
  const [pin, setPin] = useState("");
  const [pinError, setPinError] = useState<string | null>(null);

  useEffect(() => {
    ipc.localSchedules().then(setSchedules).catch((e) => setError(errorText(e)));
    const t = setInterval(() => setNow(Date.now()), 15_000);
    return () => clearInterval(t);
  }, []);

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

  const unlock = async (e: FormEvent) => {
    e.preventDefault();
    setPinError(null);
    try {
      if (await ipc.unlockOperator(pin)) {
        setPin("");
        setPinOpen(false);
        onOperator();
      } else {
        setPinError("PIN salah");
      }
    } catch (err) {
      setPinError(errorText(err));
    }
  };

  return (
    <div className="home">
      <header className="home-header">
        <div className="brand"><span className="brand-logo">CBT</span> {config.siteCode}</div>
        <div className="clock">{new Intl.DateTimeFormat("id-ID", { dateStyle: "full", timeStyle: "short" }).format(now)}</div>
      </header>

      <main className="center-screen">
        <form className="card login-card" onSubmit={submit}>
          <h1>Masuk ujian</h1>
          {schedules === null ? (
            <p className="muted">Memuat jadwal…</p>
          ) : !open.length ? (
            <Alert tone="info">
              Tidak ada ujian yang dibuka di komputer ini saat ini.
              {schedules.length ? (
                <> Jadwal berikutnya: {schedules.filter((s) => new Date(s.endAt).getTime() > now).map((s) => `${s.examTitle} (${fmtDateTime(s.startAt)})`)[0] ?? "-"}</>
              ) : (
                <> Operator perlu mengunduh paket ujian terlebih dahulu.</>
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
        <span className="muted small">{config.deviceName ?? config.deviceId.slice(0, 8)}</span>
        <button type="button" className="link-btn" onClick={() => setPinOpen(true)}>Operator</button>
      </footer>

      <Modal
        open={pinOpen}
        title="Menu operator"
        onClose={() => setPinOpen(false)}
        footer={<><Button onClick={() => setPinOpen(false)}>Batal</Button><Button variant="primary" type="submit" form="pin-form">Buka</Button></>}
      >
        <form id="pin-form" onSubmit={unlock}>
          <Field label="PIN operator">
            <Input type="password" inputMode="numeric" autoFocus value={pin} onChange={(e) => setPin(e.target.value)} />
          </Field>
          <Alert>{pinError}</Alert>
        </form>
      </Modal>
    </div>
  );
}
