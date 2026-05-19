import { lazy, Suspense, useEffect } from "react";
import { Sidebar } from "@/app/Sidebar";
import { Providers } from "@/app/providers";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { useAppStore } from "@/stores/appStore";
import { useRecordingStore } from "@/stores/recordingStore";
import { DictationPanel } from "@/features/dictation/DictationPanel";
import { ToastContainer } from "@/components/ToastContainer";
import { useToastStore } from "@/stores/toastStore";
import {
  recentHistory,
  onTranscriptionResult,
  onRecordingError,
  openMicSettings,
  onImportProgress,
  onImportTranscriptionReady,
  onImportError,
  onImportCancelled,
} from "@/lib/tauri";
import { useImportStore } from "@/stores/importStore";

// Lazy-load page components — they are only parsed/executed when navigated to,
// saving ~20-50 MB of JS heap in the main WebView window.
const HistoryPage = lazy(() =>
  import("@/features/history/HistoryPage").then((m) => ({ default: m.HistoryPage }))
);
const DictionaryPage = lazy(() =>
  import("@/features/dictionary/DictionaryPage").then((m) => ({ default: m.DictionaryPage }))
);
const ModelsPage = lazy(() =>
  import("@/features/models/ModelsPage").then((m) => ({ default: m.ModelsPage }))
);
const ContextModesPage = lazy(() =>
  import("@/features/modes/ContextModesPage").then((m) => ({ default: m.ContextModesPage }))
);
const NotesPage = lazy(() =>
  import("@/features/notes/NotesPage").then((m) => ({ default: m.NotesPage }))
);
const ImportsPage = lazy(() =>
  import("@/features/imports/ImportsPage").then((m) => ({ default: m.ImportsPage }))
);
const SettingsPage = lazy(() =>
  import("@/features/settings/SettingsPage").then((m) => ({ default: m.SettingsPage }))
);

/**
 * Always-mounted hook that keeps `lastTranscription` in sync:
 *  1. Seeds from the database on first load so the dictation page
 *     immediately shows the most recent transcription.
 *  2. Listens for the `transcription-result` event globally so hotkey
 *     dictations done while on any page are captured.
 */
function useGlobalTranscriptionSync() {
  const setLastTranscription = useRecordingStore((s) => s.setLastTranscription);
  const addToast = useToastStore((s) => s.addToast);

  // Seed from DB on mount
  useEffect(() => {
    recentHistory(1)
      .then((records) => {
        if (records.length > 0) {
          setLastTranscription(records[0].text);
        }
      })
      .catch(() => {});
  }, [setLastTranscription]);

  // Listen for new transcriptions globally (regardless of current page)
  useEffect(() => {
    const unlisten = onTranscriptionResult((text: string) => {
      setLastTranscription(text);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [setLastTranscription]);

  // Listen for pipeline errors and surface them as toasts.
  // Permission-denied errors get an action button to open System Settings.
  useEffect(() => {
    const unlisten = onRecordingError((err) => {
      if (err.code === "mic_permission_denied") {
        addToast({
          message: "Microphone access denied. Grant permission in System Settings to record audio.",
          code: err.code,
          level: "error",
          action: {
            label: "Open Settings",
            onClick: () => openMicSettings().catch(console.error),
          },
          duration: 15000,
        });
      } else {
        addToast({ message: err.message, code: err.code, level: "error" });
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [addToast]);
}

function PageRouter() {
  const currentPage = useAppStore((s) => s.currentPage);

  return (
    <Suspense
      fallback={
        <div className="flex h-full items-center justify-center">
          <div className="h-4 w-4 animate-spin rounded-full border-2 border-text-muted/25 border-t-amber-400" />
        </div>
      }
    >
      {(() => {
        switch (currentPage) {
          case "dictation":
            return <DictationPanel />;
          case "history":
            return <HistoryPage />;
          case "dictionary":
            return <DictionaryPage />;
          case "modes":
            return <ContextModesPage />;
          case "notes":
            return <NotesPage />;
          case "imports":
            return <ImportsPage />;
          case "models":
            return <ModelsPage />;
          case "settings":
            return <SettingsPage />;
          default:
            return <DictationPanel />;
        }
      })()}
    </Suspense>
  );
}

/**
 * Mount-once listeners for the Import Audio events.  Lives at the App
 * root so a long import keeps progressing even when the user navigates
 * away from the Imports page — re-mounting on the page would lose every
 * tick that fires between unmount and remount.
 *
 * Each handler filters by `payload.import_id === current.importId` so
 * late events from a cancelled prior import are dropped on the floor.
 */
function useImportEventSync() {
  useEffect(() => {
    const unlistenProgress = onImportProgress((p) => {
      const cur = useImportStore.getState().state;
      if (cur.kind !== "decoding" && cur.kind !== "transcribing") return;
      if (p.import_id !== cur.importId) return;

      if (p.phase === "decoding") {
        useImportStore.getState().applyState({
          kind: "decoding",
          importId: cur.importId,
          filename: cur.filename,
          percent: p.percent,
        });
      } else {
        if (p.duration_ms == null) {
          // Rust contract is "always set duration on transcribing-phase
          // events" — log + drop rather than guess.
          console.warn("import-progress transcribing event missing duration_ms");
          return;
        }
        useImportStore.getState().applyState({
          kind: "transcribing",
          importId: cur.importId,
          filename: cur.filename,
          percent: p.percent,
          durationMs: p.duration_ms,
        });
      }
    });
    const unlistenReady = onImportTranscriptionReady((p) => {
      const cur = useImportStore.getState().state;
      if (cur.kind !== "decoding" && cur.kind !== "transcribing") return;
      if (p.import_id !== cur.importId) return;
      useImportStore.getState().applyState({
        kind: "complete",
        importId: cur.importId,
        filename: p.source_filename,
        durationMs: p.duration_ms,
        transcript: p.text,
      });
    });
    const unlistenError = onImportError((p) => {
      const cur = useImportStore.getState().state;
      const filename =
        cur.kind === "decoding" ||
        cur.kind === "transcribing" ||
        cur.kind === "cancelling" ||
        cur.kind === "complete"
          ? cur.filename
          : null;
      // Accept any in-flight error; backend only emits when the import is alive.
      useImportStore.getState().applyState({
        kind: "error",
        importId: p.import_id,
        filename,
        message: p.message,
      });
    });
    const unlistenCancelled = onImportCancelled((p) => {
      const cur = useImportStore.getState().state;
      // Only honor the cancellation event for the import the UI thinks
      // is in flight.  Late events from a previous task are ignored.
      if (
        (cur.kind === "cancelling" ||
          cur.kind === "decoding" ||
          cur.kind === "transcribing") &&
        cur.importId === p.import_id
      ) {
        useImportStore.getState().applyState({ kind: "idle" });
      }
    });
    return () => {
      unlistenProgress.then((fn) => fn());
      unlistenReady.then((fn) => fn());
      unlistenError.then((fn) => fn());
      unlistenCancelled.then((fn) => fn());
    };
  }, []);
}

function MainApp() {
  useGlobalTranscriptionSync();
  useImportEventSync();

  return (
    <div className="flex h-screen w-screen bg-surface-0 text-text-primary">
      <Sidebar />
      <main
        className="relative flex-1 overflow-auto"
        style={{
          background:
            "radial-gradient(ellipse 90% 70% at 50% 100%, var(--color-gradient-from) 0%, var(--color-gradient-to) 70%)",
        }}
      >
        <PageRouter />
      </main>
      <ToastContainer />
    </div>
  );
}

export default function App() {
  return (
    <ErrorBoundary>
      <Providers>
        <MainApp />
      </Providers>
    </ErrorBoundary>
  );
}
