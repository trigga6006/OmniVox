import { useEffect } from "react";
import { Sidebar } from "@/app/Sidebar";
import { Providers } from "@/app/providers";
import { useAppStore } from "@/stores/appStore";
import { useRecordingStore } from "@/stores/recordingStore";
import { DictationPanel } from "@/features/dictation/DictationPanel";
import { HistoryPage } from "@/features/history/HistoryPage";
import { DictionaryPage } from "@/features/dictionary/DictionaryPage";
import { ModelsPage } from "@/features/models/ModelsPage";
import { NotesPage } from "@/features/notes/NotesPage";
import { SettingsPage } from "@/features/settings/SettingsPage";
import { FloatingPill } from "@/features/overlay/FloatingPill";
import { recentHistory, onTranscriptionResult } from "@/lib/tauri";
import { getCurrentWindow } from "@tauri-apps/api/window";

const isOverlay = getCurrentWindow().label === "overlay";

/**
 * Always-mounted hook that keeps `lastTranscription` in sync:
 *  1. Seeds from the database on first load so the dictation page
 *     immediately shows the most recent transcription.
 *  2. Listens for the `transcription-result` event globally so hotkey
 *     dictations done while on any page are captured.
 */
function useGlobalTranscriptionSync() {
  const setLastTranscription = useRecordingStore((s) => s.setLastTranscription);

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
}

function PageRouter() {
  const currentPage = useAppStore((s) => s.currentPage);

  switch (currentPage) {
    case "dictation":
      return <DictationPanel />;
    case "history":
      return <HistoryPage />;
    case "dictionary":
      return <DictionaryPage />;
    case "notes":
      return <NotesPage />;
    case "models":
      return <ModelsPage />;
    case "settings":
      return <SettingsPage />;
    default:
      return <DictationPanel />;
  }
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
            "radial-gradient(ellipse at 50% 80%, oklch(0.14 0.015 55) 0%, oklch(0.11 0.006 60) 60%)",
        }}
      >
        <PageRouter />
      </main>
    </div>
  );
}

export default function App() {
  if (isOverlay) {
    return (
      <Providers>
        <FloatingPill />
      </Providers>
    );
  }

  return (
    <Providers>
      <MainApp />
    </Providers>
  );
}
