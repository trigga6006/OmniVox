import { Mic, Clock, BarChart3, BookOpen, Command, Layers, StickyNote, BrainCircuit, Settings } from "lucide-react";
import { useAppStore, type Page } from "@/stores/appStore";
import { useRecordingStore } from "@/stores/recordingStore";
import { Logo } from "@/components/Logo";
import { cn } from "@/lib/utils";

// Main tabs, in order. Settings is rendered separately, pinned to the bottom.
const navItems: { page: Page; icon: typeof Mic; label: string }[] = [
  { page: "dictation", icon: Mic, label: "Dictation" },
  { page: "history", icon: Clock, label: "History" },
  { page: "dictionary", icon: BookOpen, label: "Dictionary" },
  { page: "commands", icon: Command, label: "Voice Commands" },
  { page: "modes", icon: Layers, label: "Modes" },
  { page: "notes", icon: StickyNote, label: "Notes" },
  { page: "models", icon: BrainCircuit, label: "Models" },
  { page: "analytics", icon: BarChart3, label: "Analytics" },
];

function NavButton({ page, icon: Icon, label }: { page: Page; icon: typeof Mic; label: string }) {
  const currentPage = useAppStore((s) => s.currentPage);
  const setPage = useAppStore((s) => s.setPage);
  const isActive = currentPage === page;

  return (
    <button
      onClick={() => setPage(page)}
      title={label}
      aria-label={label}
      aria-current={isActive ? "page" : undefined}
      className={cn(
        "relative flex h-10 w-10 items-center justify-center rounded-[10px]",
        "transition-[color,background-color] duration-200 ease-out",
        isActive
          ? "text-amber-300 bg-amber-500/[0.08]"
          : "text-text-muted hover:bg-surface-2/60 hover:text-text-secondary"
      )}
    >
      {/* Active indicator — slim amber rail on the left */}
      {isActive && (
        <span
          aria-hidden="true"
          className="absolute -left-[10px] top-1/2 h-4 w-[2.5px] -translate-y-1/2 rounded-full bg-amber-400"
        />
      )}

      <Icon size={18} strokeWidth={isActive ? 2 : 1.75} />
    </button>
  );
}

export function Sidebar() {
  const status = useRecordingStore((s) => s.status);
  const isRecording = status === "recording";

  return (
    <aside className="flex h-full w-[68px] shrink-0 flex-col items-center border-r border-border/60 bg-surface-0 py-5">
      {/* Logo */}
      <div className="flex h-9 w-9 items-center justify-center">
        <Logo size={28} />
      </div>

      {/* Separator */}
      <div className="my-5 h-px w-7 bg-border/70" />

      {/* Navigation — main tabs (flex-1 pushes Settings to the bottom) */}
      <nav className="flex flex-1 flex-col items-center gap-0.5">
        {navItems.map((item) => (
          <NavButton key={item.page} {...item} />
        ))}
      </nav>

      {/* Settings — pinned to the bottom, separated from the main tabs */}
      <div className="my-2 h-px w-7 bg-border/70" />
      <NavButton page="settings" icon={Settings} label="Settings" />

      {/* Recording status dot */}
      <div className="mt-2 flex h-6 items-center justify-center">
        {isRecording && (
          <span
            className="block h-1.5 w-1.5 rounded-full bg-recording-500"
            style={{
              boxShadow: "0 0 8px rgb(216 67 47 / 0.65)",
              animation: "breathe 2.4s ease-in-out infinite",
            }}
          />
        )}
      </div>
    </aside>
  );
}
