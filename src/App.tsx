import { useCallback, useEffect, useState } from "react";
import { errorText, ipc, type ConfigView, type ExamSession } from "./lib/ipc";
import { ExamPage } from "./pages/Exam";
import { FinishedPage } from "./pages/Finished";
import { HomePage } from "./pages/Home";
import { OperatorPage } from "./pages/Operator";
import { SetupPage } from "./pages/Setup";

type Screen =
  | { name: "loading" }
  | { name: "setup" }
  | { name: "home" }
  | { name: "operator" }
  | { name: "exam"; session: ExamSession }
  | { name: "finished"; session: ExamSession };

export function App() {
  const [screen, setScreen] = useState<Screen>({ name: "loading" });
  const [config, setConfig] = useState<ConfigView | null>(null);
  const [fatal, setFatal] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      const cfg = await ipc.getConfig();
      setConfig(cfg);
      setScreen(cfg.configured ? { name: "home" } : { name: "setup" });
    } catch (err) {
      setFatal(errorText(err));
    }
  }, []);

  useEffect(() => {
    void reload();
    // Menu klik-kanan bawaan WebView dimatikan (mis. "reload" di tengah ujian).
    const block = (e: MouseEvent) => e.preventDefault();
    window.addEventListener("contextmenu", block);
    return () => window.removeEventListener("contextmenu", block);
  }, [reload]);

  if (fatal) return <div className="center-screen"><div className="card"><h1>Aplikasi tidak dapat dijalankan</h1><p>{fatal}</p></div></div>;

  switch (screen.name) {
    case "loading":
      return <div className="center-screen muted">Memuat…</div>;
    case "setup":
      return <SetupPage config={config} onSaved={() => void reload()} />;
    case "home":
      return (
        <HomePage
          config={config!}
          onLogin={(session) => setScreen(session.status === "in_progress" ? { name: "exam", session } : { name: "finished", session })}
          onOperator={() => setScreen({ name: "operator" })}
        />
      );
    case "operator":
      return <OperatorPage config={config!} onConfigChanged={setConfig} onClose={() => setScreen({ name: "home" })} />;
    case "exam":
      return <ExamPage initial={screen.session} onFinished={(session) => setScreen({ name: "finished", session })} />;
    case "finished":
      return <FinishedPage session={screen.session} onDone={() => setScreen({ name: "home" })} />;
  }
}
