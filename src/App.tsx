import { useCallback, useEffect, useState } from "react";
import { errorText, ipc, type ConfigView, type ExamSession, type Proctor } from "./lib/ipc";
import { ExamPage } from "./pages/Exam";
import { FinishedPage } from "./pages/Finished";
import { HomePage } from "./pages/Home";
import { PairingPage } from "./pages/Pairing";
import { ParticipantMenu } from "./pages/ParticipantMenu";
import { ServerLoginPage } from "./pages/ProctorLogin";
import { ServerDashboard } from "./pages/server/Dashboard";
import { SetupPage } from "./pages/Setup";

type Screen =
  | { name: "loading" }
  | { name: "setup" }
  // server lokal
  | { name: "server-login" }
  | { name: "server"; proctor: Proctor }
  // PC peserta
  | { name: "pairing" }
  | { name: "home" }
  | { name: "participant-menu"; proctor: Proctor }
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
      if (!cfg.configured) {
        setScreen({ name: "setup" });
      } else if (cfg.mode === "server") {
        const p = await ipc.currentProctor();
        setScreen(p ? { name: "server", proctor: p } : { name: "server-login" });
      } else {
        setScreen({ name: "pairing" });
      }
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

  const toHome = useCallback(() => setScreen({ name: "home" }), []);

  if (fatal) return <div className="center-screen"><div className="card"><h1>Aplikasi tidak dapat dijalankan</h1><p>{fatal}</p></div></div>;

  switch (screen.name) {
    case "loading":
      return <div className="center-screen muted">Memuat…</div>;
    case "setup":
      return <SetupPage config={config} onSaved={() => void reload()} />;
    case "server-login":
      return <ServerLoginPage config={config!} onLoggedIn={(proctor) => setScreen({ name: "server", proctor })} />;
    case "server":
      return (
        <ServerDashboard
          config={config!}
          proctor={screen.proctor}
          onConfigChanged={setConfig}
          onLogout={() => setScreen({ name: "server-login" })}
        />
      );
    case "pairing":
      return (
        <PairingPage
          key={config?.lanUrl ?? ""}
          config={config!}
          onApproved={toHome}
          onConfigChanged={(c) => {
            setConfig(c);
            setScreen({ name: "pairing" });
          }}
        />
      );
    case "home":
      return (
        <HomePage
          config={config!}
          onLogin={(session) => setScreen(session.status === "in_progress" ? { name: "exam", session } : { name: "finished", session })}
          onProctor={(proctor) => setScreen({ name: "participant-menu", proctor })}
        />
      );
    case "participant-menu":
      return (
        <ParticipantMenu
          config={config!}
          proctor={screen.proctor}
          onConfigChanged={(c) => {
            const moved = c.lanUrl !== config?.lanUrl;
            setConfig(c);
            if (moved) setScreen({ name: "pairing" });
          }}
          onClose={toHome}
        />
      );
    case "exam":
      return <ExamPage initial={screen.session} onFinished={(session) => setScreen({ name: "finished", session })} onAbort={toHome} />;
    case "finished":
      return <FinishedPage session={screen.session} onDone={toHome} />;
  }
}
