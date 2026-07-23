import { lazy, Suspense, useEffect } from "react";
import { Sidebar } from "@/app/Sidebar";
import { Providers } from "@/app/providers";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { useAppStore } from "@/stores/appStore";
import { useRecordingStore } from "@/stores/recordingStore";
import { DictationPanel } from "@/features/dictation/DictationPanel";
import { ToastContainer } from "@/components/ToastContainer";
import { ClickPulse } from "@/components/ClickPulse";
import { Spinner } from "@/components/ui";
import { useToastStore } from "@/stores/toastStore";
import { recentHistory, onTranscriptionResult, onRecordingError, openMicSettings } from "@/lib/tauri";
import { useInAppDictation } from "@/hooks/useInAppDictation";
import { useWindowHotkeyBridge } from "@/hooks/useWindowHotkeyBridge";

// Lazy-load page components — they are only parsed/executed when navigated to,
// saving ~20-50 MB of JS heap in the main WebView window.
const HistoryPage = lazy(() =>
  import("@/features/history/HistoryPage").then((m) => ({ default: m.HistoryPage }))
);
const UserAnalyticsPage = lazy(() =>
  import("@/features/analytics/UserAnalyticsPage").then((m) => ({
    default: m.UserAnalyticsPage,
  }))
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
const VoiceCommandsPage = lazy(() =>
  import("@/features/commands/VoiceCommandsPage").then((m) => ({ default: m.VoiceCommandsPage }))
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
          <Spinner />
        </div>
      }
    >
      {(() => {
        switch (currentPage) {
          case "dictation":
            return <DictationPanel />;
          case "history":
            return <HistoryPage />;
          case "analytics":
            return <UserAnalyticsPage />;
          case "dictionary":
            return <DictionaryPage />;
          case "modes":
            return <ContextModesPage />;
          case "notes":
            return <NotesPage />;
          case "commands":
            return <VoiceCommandsPage />;
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
  useInAppDictation();
  useWindowHotkeyBridge();

  return (
    <div className="flex h-screen w-screen bg-surface-0 text-text-primary">
      <Sidebar />
      <main
        data-pulse-root
        className="relative flex-1 overflow-x-hidden overflow-y-auto"
        style={{
          background:
            "radial-gradient(ellipse 90% 70% at 50% 100%, var(--color-gradient-from) 0%, var(--color-gradient-to) 70%)",
        }}
      >
        <PageRouter />
      </main>
      <ClickPulse />
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
