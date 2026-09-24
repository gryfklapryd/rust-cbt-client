import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Html, QuestionView, type UploadedFile } from "@cbt/question-ui";
import { isEmptyResponse, type QuestionType } from "@cbt/shared";
import { Alert, Button, Modal } from "../components/ui";
import { resolveAsset } from "../lib/assets";
import { fmtDuration, fmtTime, readFileBase64 } from "../lib/format";
import { errorText, ipc, type AttemptState, type ExamSession } from "../lib/ipc";

interface LocalAnswer {
  response: unknown;
  flagged: boolean;
}

const SAVE_DEBOUNCE_MS = 500;
const VIOLATION_DEBOUNCE_MS = 2000;

export function ExamPage({ initial, onFinished }: { initial: ExamSession; onFinished: (s: ExamSession) => void }) {
  const session = initial;
  const settings = session.exam.settings;
  const attemptId = session.attemptId;

  const items = useMemo(
    () => session.sections.flatMap((s) => s.questionIds.map((qid, i) => ({ qid, section: s, firstInSection: i === 0 }))),
    [session.sections],
  );
  const [answers, setAnswers] = useState<Record<string, LocalAnswer>>(() =>
    Object.fromEntries(Object.entries(session.answers).map(([k, v]) => [k, { response: v.response, flagged: v.flagged }])),
  );
  const [index, setIndex] = useState(() => Math.min(Math.max(0, session.currentIndex), items.length - 1));
  const [intro, setIntro] = useState(() => Object.keys(session.answers).length === 0 && session.currentIndex === 0);
  const [remaining, setRemaining] = useState(session.remainingSeconds);
  const [violations, setViolations] = useState(session.violationCount);
  const [notice, setNotice] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [finishing, setFinishing] = useState(false);

  const deadlineRef = useRef(Date.now() + session.remainingSeconds * 1000);
  const timers = useRef(new Map<string, number>());
  const answersRef = useRef(answers);
  answersRef.current = answers;
  const enteredAt = useRef(Date.now());
  const indexRef = useRef(index);
  indexRef.current = index;
  const finishedRef = useRef(false);

  // ------------------------------------------------------------ akhir ujian
  const finish = useCallback(
    async (manual: boolean) => {
      if (finishedRef.current) return;
      finishedRef.current = true;
      setFinishing(true);
      // Simpan semua jawaban yang tertunda terlebih dahulu.
      for (const [qid, t] of timers.current) {
        window.clearTimeout(t);
        await persist(qid).catch(() => {});
      }
      timers.current.clear();
      try {
        await ipc.submit(attemptId, manual);
      } catch (err) {
        if (manual) {
          finishedRef.current = false;
          setFinishing(false);
          setNotice(errorText(err));
          setConfirmOpen(false);
          return;
        }
      }
      await ipc.setExamMode(false, false).catch(() => {});
      onFinished(await ipc.getSession(attemptId));
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [attemptId, onFinished],
  );

  const applyState = useCallback(
    (st: AttemptState) => {
      deadlineRef.current = Date.now() + st.remainingSeconds * 1000;
      setViolations(st.violationCount);
      if (st.status !== "in_progress") void finish(false);
    },
    [finish],
  );

  // ------------------------------------------------------------ simpan jawaban
  const persist = useCallback(
    async (qid: string) => {
      const a = answersRef.current[qid];
      if (!a) return;
      const isCurrent = items[indexRef.current]?.qid === qid;
      const delta = isCurrent ? Math.round((Date.now() - enteredAt.current) / 1000) : 0;
      if (isCurrent) enteredAt.current = Date.now();
      try {
        const st = await ipc.saveAnswer({
          attemptId,
          questionId: qid,
          response: a.response ?? null,
          flagged: a.flagged,
          timeSpentDelta: delta,
          currentIndex: indexRef.current,
        });
        setSaveError(null);
        applyState(st);
      } catch (err) {
        setSaveError(errorText(err));
      }
    },
    [attemptId, items, applyState],
  );

  const schedule = useCallback(
    (qid: string, delay = SAVE_DEBOUNCE_MS) => {
      const prev = timers.current.get(qid);
      if (prev) window.clearTimeout(prev);
      timers.current.set(
        qid,
        window.setTimeout(() => {
          timers.current.delete(qid);
          void persist(qid);
        }, delay),
      );
    },
    [persist],
  );

  const update = (qid: string, patch: Partial<LocalAnswer>) => {
    setAnswers((prev) => {
      const next = { ...prev, [qid]: { response: prev[qid]?.response ?? null, flagged: prev[qid]?.flagged ?? false, ...patch } };
      answersRef.current = next;
      return next;
    });
    schedule(qid);
  };

  const goTo = (i: number) => {
    if (i < 0 || i >= items.length) return;
    if (!settings.allowBackNavigation && i < index) return;
    const current = items[index]?.qid;
    // Simpan waktu pengerjaan soal yang ditinggalkan.
    if (current) {
      const t = timers.current.get(current);
      if (t) window.clearTimeout(t);
      timers.current.delete(current);
      void persist(current);
    }
    enteredAt.current = Date.now();
    setIndex(i);
  };

  // ------------------------------------------------------------ mode ujian, timer, pelanggaran
  useEffect(() => {
    void ipc.setExamMode(true, settings.lockdown);
    const tick = window.setInterval(() => {
      const left = Math.max(0, Math.round((deadlineRef.current - Date.now()) / 1000));
      setRemaining(left);
      if (left <= 0) void finish(false);
    }, 1000);

    let lastViolation = 0;
    const violation = (reason: string) => {
      if (finishedRef.current || !settings.lockdown) return;
      const now = Date.now();
      if (now - lastViolation < VIOLATION_DEBOUNCE_MS) return;
      lastViolation = now;
      ipc
        .logEvent(attemptId, "violation", { reason })
        .then((st) => {
          applyState(st);
          if (st.status === "in_progress") {
            setNotice(
              settings.maxViolations > 0
                ? `Peringatan: Anda meninggalkan layar ujian (${st.violationCount}/${settings.maxViolations}). Ujian akan dihentikan bila batas tercapai.`
                : "Peringatan: Anda meninggalkan layar ujian. Kejadian ini dicatat.",
            );
          }
        })
        .catch(() => {});
    };
    const onBlur = () => violation("focus_lost");
    const onVisibility = () => document.visibilityState === "hidden" && violation("hidden");
    const onKey = (e: KeyboardEvent) => {
      const k = e.key.toLowerCase();
      // Blokir muat ulang, cetak, simpan, tutup tab, dan devtools.
      if (k === "f5" || k === "f12" || ((e.ctrlKey || e.metaKey) && ["r", "p", "s", "w", "u", "i"].includes(k))) e.preventDefault();
    };
    window.addEventListener("blur", onBlur);
    document.addEventListener("visibilitychange", onVisibility);
    window.addEventListener("keydown", onKey);
    const unlisten = listen("close-blocked", () => setNotice("Aplikasi tidak bisa ditutup selama ujian berlangsung."));
    return () => {
      window.clearInterval(tick);
      window.removeEventListener("blur", onBlur);
      document.removeEventListener("visibilitychange", onVisibility);
      window.removeEventListener("keydown", onKey);
      void unlisten.then((f) => f());
    };
  }, [attemptId, settings.lockdown, settings.maxViolations, applyState, finish]);

  // ------------------------------------------------------------ tampilan
  const item = items[index];
  const q = item ? session.questions[item.qid] : undefined;
  const answer = item ? answers[item.qid] : undefined;
  const stimulus = q?.stimulusId ? session.stimuli[q.stimulusId] : undefined;
  const answered = (qid: string) => {
    const quest = session.questions[qid];
    return !!quest && !isEmptyResponse(quest.type as QuestionType, answers[qid]?.response ?? null);
  };
  const answeredCount = items.filter((it) => answered(it.qid)).length;
  const flaggedCount = items.filter((it) => answers[it.qid]?.flagged).length;
  const canSubmitAt = new Date(session.canSubmitAt).getTime();
  const lowTime = remaining <= 300;

  const onUploadFile = async (file: File): Promise<UploadedFile> => {
    const data = await readFileBase64(file);
    return ipc.saveAttachment(attemptId, item!.qid, file.name, file.type, data);
  };

  return (
    <div className="exam">
      <header className="exam-header">
        <div className="exam-title">
          <strong>{session.exam.title}</strong>
          <span className="muted small">{session.participant.number} · {session.participant.name}</span>
        </div>
        <div className={`timer${lowTime ? " timer-low" : ""}`} role="timer" aria-label="Sisa waktu">
          {fmtDuration(remaining)}
        </div>
      </header>

      {notice ? (
        <div className="exam-notice" role="alert">
          <span>{notice}</span>
          <button type="button" className="link-btn" onClick={() => setNotice(null)}>Tutup</button>
        </div>
      ) : null}
      {saveError ? <div className="exam-notice exam-notice-danger" role="alert">Gagal menyimpan jawaban: {saveError}</div> : null}

      {intro ? (
        <main className="exam-intro">
          <div className="card">
            <h1>{session.exam.title}</h1>
            <dl className="kv">
              <dt>Peserta</dt><dd>{session.participant.number} · {session.participant.name}</dd>
              <dt>Jumlah soal</dt><dd>{items.length}</dd>
              <dt>Waktu</dt><dd>{session.exam.durationMinutes} menit, berakhir pukul {fmtTime(new Date(deadlineRef.current).toISOString())}</dd>
            </dl>
            {session.exam.instructions ? <Html className="instructions" html={session.exam.instructions} resolveAsset={resolveAsset} /> : null}
            <ul className="rules">
              <li>Jawaban tersimpan otomatis setiap kali Anda menjawab.</li>
              {settings.allowFlagging ? <li>Tandai soal dengan <b>Ragu-ragu</b> bila ingin memeriksanya kembali.</li> : null}
              {!settings.allowBackNavigation ? <li>Anda <b>tidak dapat kembali</b> ke soal sebelumnya.</li> : null}
              {settings.lockdown ? <li>Jangan meninggalkan layar ujian; setiap kejadian dicatat{settings.maxViolations > 0 ? ` dan ujian dihentikan setelah ${settings.maxViolations} kali` : ""}.</li> : null}
            </ul>
            <Button variant="primary" className="btn-lg" onClick={() => { enteredAt.current = Date.now(); setIntro(false); }}>
              Mulai mengerjakan
            </Button>
          </div>
        </main>
      ) : item && q ? (
        <div className="exam-body">
          <main className={`exam-main${stimulus ? " with-stimulus" : ""}`}>
            {stimulus ? (
              <aside className="stimulus">
                <h3>{stimulus.title}</h3>
                <Html html={stimulus.content} resolveAsset={resolveAsset} />
              </aside>
            ) : null}
            <section className="question-panel">
              {item.firstInSection && item.section.instructions ? (
                <Html className="section-instructions" html={item.section.instructions} resolveAsset={resolveAsset} />
              ) : null}
              <div className="question-head">
                <span className="question-number">Soal {index + 1} dari {items.length}</span>
                <span className="muted small">{item.section.title}</span>
              </div>
              <QuestionView
                key={item.qid}
                type={q.type}
                content={q.content}
                value={answer?.response ?? null}
                onChange={(response) => update(item.qid, { response })}
                resolveAsset={resolveAsset}
                order={session.optionOrders[item.qid]}
                onUploadFile={onUploadFile}
              />
            </section>
          </main>

          <aside className="exam-nav">
            <div className="nav-summary">
              <span>{answeredCount}/{items.length} dijawab</span>
              {flaggedCount ? <span className="flag-count">{flaggedCount} ragu</span> : null}
              {violations ? <span className="violation-count">{violations} pelanggaran</span> : null}
            </div>
            <div className="nav-grid">
              {items.map((it, i) => (
                <button
                  key={it.qid}
                  type="button"
                  className={`nav-cell${i === index ? " is-current" : ""}${answered(it.qid) ? " is-answered" : ""}${answers[it.qid]?.flagged ? " is-flagged" : ""}`}
                  disabled={!settings.allowBackNavigation && i < index}
                  onClick={() => goTo(i)}
                >
                  {i + 1}
                </button>
              ))}
            </div>
            <div className="nav-legend small muted">
              <span><i className="lg lg-answered" /> dijawab</span>
              <span><i className="lg lg-flagged" /> ragu-ragu</span>
            </div>
          </aside>
        </div>
      ) : null}

      {!intro ? (
        <footer className="exam-footer">
          <Button disabled={index === 0 || !settings.allowBackNavigation} onClick={() => goTo(index - 1)}>‹ Sebelumnya</Button>
          {settings.allowFlagging && item ? (
            <label className={`flag-toggle${answer?.flagged ? " is-on" : ""}`}>
              <input type="checkbox" checked={!!answer?.flagged} onChange={(e) => update(item.qid, { flagged: e.target.checked })} />
              Ragu-ragu
            </label>
          ) : <span />}
          {index < items.length - 1 ? (
            <Button variant="primary" onClick={() => goTo(index + 1)}>Berikutnya ›</Button>
          ) : (
            <Button variant="primary" onClick={() => setConfirmOpen(true)}>Selesai</Button>
          )}
        </footer>
      ) : null}

      <Modal
        open={confirmOpen}
        title="Kumpulkan ujian?"
        onClose={() => setConfirmOpen(false)}
        footer={
          <>
            <Button onClick={() => setConfirmOpen(false)}>Periksa lagi</Button>
            <Button variant="primary" loading={finishing} disabled={Date.now() < canSubmitAt} onClick={() => void finish(true)}>
              Ya, kumpulkan
            </Button>
          </>
        }
      >
        <p>Setelah dikumpulkan, jawaban tidak dapat diubah lagi.</p>
        <ul>
          <li>{answeredCount} dari {items.length} soal dijawab</li>
          {items.length - answeredCount > 0 ? <li className="text-warning">{items.length - answeredCount} soal belum dijawab</li> : null}
          {flaggedCount ? <li className="text-warning">{flaggedCount} soal masih ditandai ragu-ragu</li> : null}
        </ul>
        {Date.now() < canSubmitAt ? <Alert tone="info">Ujian baru bisa dikumpulkan setelah pukul {fmtTime(session.canSubmitAt)}.</Alert> : null}
      </Modal>
    </div>
  );
}
