import { useEffect, useState } from "react";
import { Button } from "../components/ui";
import { fmtDateTime } from "../lib/format";
import type { ExamSession } from "../lib/ipc";

const MESSAGES: Record<string, string> = {
  submitted: "Jawaban Anda sudah dikumpulkan.",
  timed_out: "Waktu ujian habis. Jawaban Anda dikumpulkan otomatis.",
  terminated: "Ujian dihentikan karena batas pelanggaran terlampaui. Silakan hubungi pengawas.",
};

/** Layar penutup; kembali otomatis ke layar login agar komputer siap untuk peserta berikutnya. */
export function FinishedPage({ session, onDone }: { session: ExamSession; onDone: () => void }) {
  const [left, setLeft] = useState(20);
  useEffect(() => {
    const t = setInterval(() => setLeft((n) => n - 1), 1000);
    return () => clearInterval(t);
  }, []);
  useEffect(() => {
    if (left <= 0) onDone();
  }, [left, onDone]);

  const answered = Object.values(session.answers).filter((a) => a.response !== null).length;
  return (
    <div className="center-screen">
      <div className={`card finished-card${session.status === "terminated" ? " is-danger" : ""}`}>
        <div className="finished-icon" aria-hidden>{session.status === "terminated" ? "!" : "✓"}</div>
        <h1>{session.status === "terminated" ? "Ujian dihentikan" : "Ujian selesai"}</h1>
        <p>{MESSAGES[session.status] ?? "Ujian sudah selesai."}</p>
        <dl className="kv">
          <dt>Peserta</dt><dd>{session.participant.number} · {session.participant.name}</dd>
          <dt>Ujian</dt><dd>{session.exam.title}</dd>
          <dt>Selesai</dt><dd>{fmtDateTime(session.finishedAt)}</dd>
          <dt>Soal dijawab</dt><dd>{answered}</dd>
        </dl>
        <p className="muted small">Nilai akan diumumkan setelah hasil diproses di server pusat.</p>
        <Button variant="primary" onClick={onDone}>Selesai ({left})</Button>
      </div>
    </div>
  );
}
