import { lazy, Suspense, useEffect } from "react";
import { Sidebar } from "@/app/Sidebar";
import { Providers } from "@/app/providers";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { useAppStore } from "@/stores/appStore";
import { useRecordingStore } from "@/stores/recordingStore";
import { DictationPanel } from "@/features/dictation/DictationPanel";
import { ToastContainer } from "@/components/ToastContainer";
import { useToastStore } from "@/stores/toastStore";
import { recentHistory, onTranscriptionResult, onRecordingError } from "@/lib/tauri";

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

  // Listen for pipeline errors and surface them as toasts
  useEffect(() => {
    const unlisten = onRecordingError((err) => {
      addToast({ message: err.message, code: err.code, level: "error" });
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
          <div className="h-5 w-5 animate-spin rounded-full border-2 border-text-muted/20 border-t-amber-500" />
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

function MainApp() {
  useGlobalTranscriptionSync();

  return (
    <div className="flex h-screen w-screen bg-surface-0">
      <Sidebar />
      <main
        className="flex-1 overflow-auto"
        style={{
          background:
            "radial-gradient(ellipse at 50% 80%, var(--color-gradient-from) 0%, var(--color-gradient-to) 60%)",
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
