import {
  Mic,
  Clock,
  BarChart3,
  BookOpen,
  Layers,
  StickyNote,
  Command,
  BrainCircuit,
  Settings,
  PanelLeftClose,
  PanelLeftOpen,
} from "lucide-react";
import { useAppStore, type Page } from "@/stores/appStore";
import { useRecordingStore } from "@/stores/recordingStore";
import { Logo } from "@/components/Logo";
import { cn } from "@/lib/utils";

// Main tabs, in order. Settings is rendered separately, pinned to the bottom.
const navItems: { page: Page; icon: typeof Mic; label: string }[] = [
  { page: "dictation", icon: Mic, label: "Dictation" },
  { page: "history", icon: Clock, label: "History" },
  { page: "dictionary", icon: BookOpen, label: "Dictionary" },
  { page: "modes", icon: Layers, label: "Modes" },
  { page: "notes", icon: StickyNote, label: "Notes" },
  { page: "commands", icon: Command, label: "Commands" },
  { page: "models", icon: BrainCircuit, label: "Models" },
  { page: "analytics", icon: BarChart3, label: "Analytics" },
];

function NavButton({
  page,
  icon: Icon,
  label,
  collapsed,
}: {
  page: Page;
  icon: typeof Mic;
  label: string;
  collapsed: boolean;
}) {
  const currentPage = useAppStore((s) => s.currentPage);
  const setPage = useAppStore((s) => s.setPage);
  const isActive = currentPage === page;

  return (
    <button
      onClick={() => setPage(page)}
      title={collapsed ? label : undefined}
      aria-label={label}
      aria-current={isActive ? "page" : undefined}
      className={cn(
        "relative flex h-10 items-center rounded-xl",
        "transition-[color,background-color] duration-200 ease-out",
        collapsed ? "w-10 justify-center" : "w-full gap-3 px-3",
        isActive
          ? "text-amber-300 bg-amber-500/[0.08]"
          : "text-text-muted hover:bg-surface-2 hover:text-text-secondary"
      )}
    >
      {/* Active indicator — slim amber rail on the left */}
      {isActive && (
        <span
          aria-hidden="true"
          className="absolute -left-[10px] top-1/2 h-4 w-[2.5px] -translate-y-1/2 rounded-full bg-amber-400"
        />
      )}

      <Icon size={18} strokeWidth={isActive ? 2 : 1.75} className="shrink-0" />
      {!collapsed && <span className="truncate text-sm font-medium">{label}</span>}
    </button>
  );
}

export function Sidebar() {
  const status = useRecordingStore((s) => s.status);
  const isRecording = status === "recording";
  const collapsed = useAppStore((s) => s.sidebarCollapsed);
  const toggleSidebar = useAppStore((s) => s.toggleSidebar);

  return (
    <aside
      className={cn(
        "flex h-full shrink-0 flex-col border-r border-border bg-surface-0 py-5",
        "transition-[width] duration-200 ease-out",
        collapsed ? "w-[68px] items-center px-3.5" : "w-[220px] px-3"
      )}
    >
      {/* Header — logo + name, with the collapse/expand control */}
      {collapsed ? (
        // Collapsed: logo centered; hovering swaps it for the expand button.
        <button
          onClick={toggleSidebar}
          title="Expand sidebar"
          aria-label="Expand sidebar"
          className="group relative flex h-9 w-9 items-center justify-center rounded-lg hover:bg-surface-2"
        >
          <Logo size={28} className="transition-opacity duration-150 group-hover:opacity-0" />
          <PanelLeftOpen
            size={18}
            strokeWidth={1.75}
            aria-hidden="true"
            className="absolute text-text-secondary opacity-0 transition-opacity duration-150 group-hover:opacity-100"
          />
        </button>
      ) : (
        // Expanded: logo + name on the left, minimize button on the right.
        <div className="flex h-9 items-center justify-between px-1">
          <div className="flex items-center gap-2.5">
            <Logo size={26} />
            <span className="text-[15px] font-semibold tracking-tight text-text-primary">
              OmniVox
            </span>
          </div>
          <button
            onClick={toggleSidebar}
            title="Collapse sidebar"
            aria-label="Collapse sidebar"
            className="flex h-8 w-8 items-center justify-center rounded-lg text-text-muted transition-colors hover:bg-surface-2 hover:text-text-secondary"
          >
            <PanelLeftClose size={18} strokeWidth={1.75} />
          </button>
        </div>
      )}

      {/* Separator */}
      <div className={cn("my-5 h-px bg-border", collapsed ? "w-7" : "w-full")} />

      {/* Navigation — main tabs (flex-1 pushes Settings to the bottom) */}
      <nav className={cn("flex flex-1 flex-col gap-0.5", collapsed && "items-center")}>
        {navItems.map((item) => (
          <NavButton key={item.page} {...item} collapsed={collapsed} />
        ))}
      </nav>

      {/* Settings — pinned to the bottom, separated from the main tabs */}
      <div className={cn("my-2 h-px bg-border", collapsed ? "w-7" : "w-full")} />
      <NavButton page="settings" icon={Settings} label="Settings" collapsed={collapsed} />

      {/* Recording status dot */}
      <div className="mt-2 flex h-6 items-center justify-center">
        {isRecording && (
          <span
            className="block h-1.5 w-1.5 rounded-full bg-recording-500"
            style={{
              animation: "breathe 2.4s ease-in-out infinite",
            }}
          />
        )}
      </div>
    </aside>
  );
}
