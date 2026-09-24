import { useCallback, useEffect, useMemo, useState } from "react";
import { Alert, Badge, Button, Field, Input, Modal, StatusBadge } from "../../components/ui";
import { fmtDateTime, fmtDuration, fmtTime } from "../../lib/format";
import { errorText, ipc, type LocalSchedule, type MonitorRow } from "../../lib/ipc";
import type { Notify } from "./Dashboard";

const REFRESH_MS = 5_000;
/** PC dianggap terhubung bila menghubungi server lokal dalam 30 detik terakhir. */
const ONLINE_MS = 30_000;

type Action = "time" | "terminate" | "unlock" | "move" | "delete";

/** Jadwal yang paling relevan: sedang berlangsung, lalu yang berikutnya. */
function pickSchedule(list: LocalSchedule[], now: number): string {
  const running = list.find((s) => new Date(s.startAt).getTime() <= now && now < new Date(s.endAt).getTime());
  const next = list.find((s) => new Date(s.endAt).getTime() > now);
  return (running ?? next ?? list[list.length - 1])?.scheduleId ?? "";
}

export function MonitorTab({ notify }: { notify: Notify }) {
  const [schedules, setSchedules] = useState<LocalSchedule[] | null>(null);
  const [scheduleId, setScheduleId] = useState("");
  const [rows, setRows] = useState<MonitorRow[]>([]);
  const [filter, setFilter] = useState("");
  const [dialog, setDialog] = useState<{ action: Action; row: MonitorRow } | null>(null);

  useEffect(() => {
    ipc
      .localSchedules()
      .then((l) => {
        setSchedules(l);
        setScheduleId((cur) => cur || pickSchedule(l, Date.now()));
      })
      .catch((e) => notify({ tone: "danger", text: errorText(e) }));
  }, [notify]);

  const load = useCallback(async () => {
    if (!scheduleId) return;
    setRows(await ipc.monitor(scheduleId));
  }, [scheduleId]);

  useEffect(() => {
    load().catch((e) => notify({ tone: "danger", text: errorText(e) }));
    const t = setInterval(() => void load().catch(() => {}), REFRESH_MS);
    return () => clearInterval(t);
  }, [load, notify]);

  const schedule = schedules?.find((s) => s.scheduleId === scheduleId);
  const now = Date.now();
  const online = (r: MonitorRow) => !!r.deviceLastSeen && now - new Date(r.deviceLastSeen).getTime() < ONLINE_MS;
  const counts = useMemo(() => {
    const c = { not_started: 0, in_progress: 0, finished: 0, terminated: 0, offline: 0 };
    for (const r of rows) {
      if (r.status === "not_started") c.not_started++;
      else if (r.status === "in_progress") {
        c.in_progress++;
        if (!online(r)) c.offline++;
      } else if (r.status === "terminated") c.terminated++;
      else c.finished++;
    }
    return c;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rows]);
  const q = filter.trim().toLowerCase();
  const shown = q ? rows.filter((r) => r.number.toLowerCase().includes(q) || r.name.toLowerCase().includes(q) || (r.deviceName ?? "").toLowerCase().includes(q)) : rows;

  if (schedules && !schedules.length) {
    return <Alert tone="info">Belum ada paket ujian di server lokal. Buka tab <b>Paket ujian</b> untuk mengunduh dari server pusat.</Alert>;
  }

  return (
    <div className="stack">
      <section className="card">
        <div className="card-header">
          <select className="input input-inline" value={scheduleId} onChange={(e) => setScheduleId(e.target.value)}>
            {(schedules ?? []).map((s) => (
              <option key={s.scheduleId} value={s.scheduleId}>
                {s.examTitle} · {s.name} ({fmtDateTime(s.startAt)})
              </option>
            ))}
          </select>
          {schedule ? (
            <div className="token-box" title="Bacakan token ini kepada peserta saat ujian dimulai">
              <span className="small muted">Token sesi</span>
              <strong className="mono">{schedule.requiresToken ? schedule.accessToken ?? "?" : "tanpa token"}</strong>
            </div>
          ) : null}
        </div>
        {schedule ? (
          <p className="muted small">
            {fmtDateTime(schedule.startAt)} s.d. {fmtTime(schedule.endAt)} · {schedule.durationMinutes} menit · {schedule.questionCount} soal ·
            paket v{schedule.packageVersion}
            {schedule.requiresToken && !schedule.accessToken ? " · token belum diketahui, buka tab Paket ujian saat online" : ""}
          </p>
        ) : null}
        <div className="stats">
          <div className="stat"><span>Belum login</span><strong>{counts.not_started}</strong></div>
          <div className="stat"><span>Sedang ujian</span><strong>{counts.in_progress}</strong></div>
          <div className="stat"><span>Selesai</span><strong>{counts.finished}</strong></div>
          <div className={`stat${counts.terminated ? " stat-danger" : ""}`}><span>Dihentikan</span><strong>{counts.terminated}</strong></div>
          <div className={`stat${counts.offline ? " stat-warning" : ""}`}><span>PC terputus</span><strong>{counts.offline}</strong></div>
        </div>
      </section>

      <section className="card">
        <div className="card-header">
          <h2>Peserta</h2>
          <input className="input input-inline" placeholder="Cari nomor, nama, atau PC…" value={filter} onChange={(e) => setFilter(e.target.value)} />
        </div>
        <table className="table">
          <thead>
            <tr><th>Peserta</th><th>Status</th><th>PC</th><th>Dijawab</th><th>Sisa waktu</th><th>Pelanggaran</th><th /></tr>
          </thead>
          <tbody>
            {shown.map((r) => (
              <tr key={r.participantId} className={r.status === "in_progress" && !online(r) ? "row-warning" : undefined}>
                <td><span className="mono">{r.number}</span> {r.name}{r.groupName ? <div className="muted small">{r.groupName}</div> : null}</td>
                <td>
                  {r.status === "not_started" ? <Badge>belum login</Badge> : <StatusBadge status={r.status} />}
                  {r.attemptId && !r.synced ? <div className="muted small">belum terkirim ke pusat</div> : null}
                  {r.syncError ? <div className="text-danger small">ditolak pusat: {r.syncError}</div> : null}
                </td>
                <td className="small">
                  {r.deviceName ? (
                    <>
                      <span className={`dot ${online(r) ? "dot-on" : "dot-off"}`} /> {r.deviceName}
                      {r.status === "in_progress" && !online(r) ? <div className="text-warning">terputus sejak {fmtTime(r.deviceLastSeen)}</div> : null}
                    </>
                  ) : r.attemptId ? <span className="muted">boleh pindah PC</span> : "-"}
                </td>
                <td>{r.attemptId ? `${r.answered}/${r.questionCount}` : "-"}</td>
                <td className="mono">{r.status === "in_progress" ? fmtDuration(r.remainingSeconds) : "-"}</td>
                <td>{r.violationCount ? <Badge tone="warning">{r.violationCount}</Badge> : r.attemptId ? 0 : "-"}</td>
                <td className="row-actions">
                  {r.status === "in_progress" ? (
                    <>
                      <Button variant="ghost" onClick={() => setDialog({ action: "time", row: r })}>+ Waktu</Button>
                      {r.deviceId ? <Button variant="ghost" onClick={() => setDialog({ action: "move", row: r })}>Pindah PC</Button> : null}
                      <Button variant="ghost" onClick={() => setDialog({ action: "terminate", row: r })}>Hentikan</Button>
                    </>
                  ) : r.attemptId ? (
                    <>
                      <Button variant="ghost" onClick={() => setDialog({ action: "unlock", row: r })}>Buka kunci</Button>
                      <Button variant="ghost" onClick={() => setDialog({ action: "delete", row: r })}>Hapus</Button>
                    </>
                  ) : null}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>

      {dialog ? (
        <ActionDialog
          action={dialog.action}
          row={dialog.row}
          onClose={() => setDialog(null)}
          onDone={(text) => {
            setDialog(null);
            notify({ tone: "success", text });
            void load();
          }}
        />
      ) : null}
    </div>
  );
}

const TITLES: Record<Action, string> = {
  time: "Tambah waktu",
  terminate: "Hentikan ujian peserta",
  unlock: "Buka kunci peserta",
  move: "Izinkan pindah komputer",
  delete: "Hapus data ujian peserta",
};

function ActionDialog({ action, row, onClose, onDone }: { action: Action; row: MonitorRow; onClose: () => void; onDone: (msg: string) => void }) {
  const [minutes, setMinutes] = useState(action === "time" ? "10" : row.remainingSeconds > 0 ? "0" : "10");
  const [reason, setReason] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const who = `${row.number} ${row.name}`;

  const run = async () => {
    const id = row.attemptId!;
    setBusy(true);
    setError(null);
    try {
      switch (action) {
        case "time":
          await ipc.extendTime(id, Number(minutes));
          onDone(`Waktu ${who} ditambah ${minutes} menit.`);
          break;
        case "terminate":
          await ipc.terminateAttempt(id, reason || undefined);
          onDone(`Ujian ${who} dihentikan.`);
          break;
        case "unlock":
          await ipc.unlockAttempt(id, Number(minutes) || undefined);
          onDone(`${who} bisa melanjutkan ujian.`);
          break;
        case "move":
          await ipc.releaseDevice(id);
          onDone(`${who} bisa login di komputer lain dan melanjutkan dari jawaban terakhir.`);
          break;
        case "delete":
          await ipc.deleteAttempt(id);
          onDone(`Data ujian ${who} dihapus dari server lokal.`);
          break;
      }
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  return (
    <Modal
      open
      title={TITLES[action]}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Batal</Button>
          <Button variant={action === "terminate" || action === "delete" ? "danger" : "primary"} loading={busy} onClick={() => void run()}>
            {TITLES[action]}
          </Button>
        </>
      }
    >
      <p><strong>{who}</strong>{row.deviceName ? <span className="muted"> · {row.deviceName}</span> : null}</p>
      {action === "time" ? (
        <Field label="Tambahan waktu (menit)" hint="Boleh negatif untuk mengoreksi">
          <Input type="number" value={minutes} onChange={(e) => setMinutes(e.target.value)} autoFocus />
        </Field>
      ) : null}
      {action === "unlock" ? (
        <Field label="Tambahan waktu (menit)" hint={row.remainingSeconds > 0 ? "Sisa waktu peserta masih ada; boleh 0" : "Waktu peserta sudah habis, isi tambahan waktu"}>
          <Input type="number" min={0} value={minutes} onChange={(e) => setMinutes(e.target.value)} autoFocus />
        </Field>
      ) : null}
      {action === "terminate" ? (
        <Field label="Alasan (opsional)" hint="Tercatat di log dan dikirim ke server pusat">
          <Input value={reason} onChange={(e) => setReason(e.target.value)} autoFocus />
        </Field>
      ) : null}
      {action === "move" ? (
        <p className="muted">
          Pakai bila komputer peserta rusak atau mati. Peserta login lagi di komputer lain dengan nomor, password, dan token yang sama,
          lalu melanjutkan dengan jawaban dan sisa waktu terakhir. Komputer lama tidak bisa lagi mengirim jawaban untuk ujian ini.
        </p>
      ) : null}
      {action === "delete" ? (
        <Alert tone="warning">
          Semua jawaban peserta ini di server lokal dihapus dan peserta bisa mulai dari awal. Bila hasilnya sudah terkirim ke server pusat,
          admin pusat juga harus menghapusnya di sana; kalau tidak, hasil baru akan ditolak.
        </Alert>
      ) : null}
      <Alert>{error}</Alert>
    </Modal>
  );
}
