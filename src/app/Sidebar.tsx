import { Mic, Clock, BookOpen, Layers, StickyNote, BrainCircuit, Settings } from "lucide-react";
import { useAppStore, type Page } from "@/stores/appStore";
import { useRecordingStore } from "@/stores/recordingStore";
import { Logo } from "@/components/Logo";
import { cn } from "@/lib/utils";

const navItems: { page: Page; icon: typeof Mic; label: string }[] = [
  { page: "dictation", icon: Mic, label: "Dictation" },
  { page: "history", icon: Clock, label: "History" },
  { page: "dictionary", icon: BookOpen, label: "Dictionary" },
  { page: "modes", icon: Layers, label: "Modes" },
  { page: "notes", icon: StickyNote, label: "Notes" },
  { page: "models", icon: BrainCircuit, label: "Models" },
  { page: "settings", icon: Settings, label: "Settings" },
];

export function Sidebar() {
  const { currentPage, setPage } = useAppStore();
  const status = useRecordingStore((s) => s.status);
  const isRecording = status === "recording";

  return (
    <aside className="flex h-full w-[72px] shrink-0 flex-col items-center border-r border-border bg-surface-0 py-5">
      {/* Logo */}
      <Logo size={32} />

      {/* Separator */}
      <div className="my-4 h-px w-8 bg-surface-3" />

      {/* Navigation */}
      <nav className="flex flex-1 flex-col items-center gap-1">
        {navItems.map(({ page, icon: Icon, label }) => {
          const isActive = currentPage === page;

          return (
            <button
              key={page}
              onClick={() => setPage(page)}
              title={label}
              aria-label={label}
              aria-current={isActive ? "page" : undefined}
              className={cn(
                "relative flex h-10 w-10 items-center justify-center rounded-xl transition-colors duration-200",
                isActive
                  ? "text-amber-400"
                  : "text-text-muted hover:bg-surface-2 hover:text-text-secondary",
              )}
            >
              {/* Active indicator — 3px amber bar on the left */}
              {isActive && (
                <span className="absolute left-0 top-1/2 h-5 w-[3px] -translate-y-1/2 rounded-r-full bg-amber-500" />
              )}

              <Icon size={20} strokeWidth={1.75} />
            </button>
          );
        })}
      </nav>

      {/* Recording status dot */}
      <div className="flex h-6 items-center justify-center">
        {isRecording && (
          <span className="block h-1.5 w-1.5 rounded-full bg-recording-500 animate-breathe" />
        )}
      </div>
    </aside>
  );
}
