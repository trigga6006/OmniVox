import { Sidebar } from "@/app/Sidebar";
import { Providers } from "@/app/providers";
import { useAppStore } from "@/stores/appStore";
import { DictationPanel } from "@/features/dictation/DictationPanel";
import { HistoryPage } from "@/features/history/HistoryPage";
import { DictionaryPage } from "@/features/dictionary/DictionaryPage";
import { ModelsPage } from "@/features/models/ModelsPage";
import { SettingsPage } from "@/features/settings/SettingsPage";

function PageRouter() {
  const currentPage = useAppStore((s) => s.currentPage);

  switch (currentPage) {
    case "dictation":
      return <DictationPanel />;
    case "history":
      return <HistoryPage />;
    case "dictionary":
      return <DictionaryPage />;
    case "models":
      return <ModelsPage />;
    case "settings":
      return <SettingsPage />;
    default:
      return <DictationPanel />;
  }
}

export default function App() {
  return (
    <Providers>
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
    </Providers>
  );
}
