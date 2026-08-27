import {
  Mic,
  Clock,
  BarChart3,
  BookOpen,
  Layers,
  StickyNote,
  SquareTerminal,
  BrainCircuit,
  Settings,
  PanelLeftClose,
  PanelLeftOpen,
  AudioLines,
} from "lucide-react";
import { useAppStore, type Page } from "@/stores/appStore";
import { useRecordingStore } from "@/stores/recordingStore";
import { Logo } from "@/components/Logo";
import { Tooltip } from "@/components/ui";
import { cn } from "@/lib/utils";

interface NavItem {
  page: Page;
  icon: typeof Mic;
  label: string;
}

/**
 * Three labelled groups instead of one nine-item run: what you capture with,
 * what you look things up in, what you configure. Settings is rendered
 * separately, pinned to the bottom.
 */
const navGroups: { label: string; items: NavItem[] }[] = [
  {
    label: "Capture",
    items: [
      { page: "dictation", icon: Mic, label: "Dictation" },
      { page: "meetings", icon: AudioLines, label: "Meetings" },
    ],
  },
  {
    label: "Library",
    items: [
      { page: "history", icon: Clock, label: "History" },
      { page: "notes", icon: StickyNote, label: "Notes" },
      { page: "dictionary", icon: BookOpen, label: "Dictionary" },
    ],
  },
  {
    label: "System",
    items: [
      { page: "modes", icon: Layers, label: "Modes" },
      // SquareTerminal, not ⌘ — the Command glyph is macOS iconography on a
      // Windows-first app.
      { page: "commands", icon: SquareTerminal, label: "Commands" },
      { page: "models", icon: BrainCircuit, label: "Models" },
      { page: "analytics", icon: BarChart3, label: "Analytics" },
    ],
  },
];

function NavButton({
  page,
  icon: Icon,
  label,
  collapsed,
}: NavItem & { collapsed: boolean }) {
  const currentPage = useAppStore((s) => s.currentPage);
  const setPage = useAppStore((s) => s.setPage);
  const isActive = currentPage === page;

  const button = (
    <button
      onClick={() => setPage(page)}
      aria-label={label}
      aria-current={isActive ? "page" : undefined}
      className={cn(
        "pressable relative flex h-[var(--control-h-l)] items-center rounded-[var(--radius-m)]",
        "transition-[color,background-color,transform] duration-[var(--dur-2)] ease-out",
        collapsed ? "w-10 justify-center" : "w-full gap-3 px-3",
        isActive
          ? "bg-amber-500/[0.08] text-amber-300"
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

  // Collapsed rail: the kit Tooltip carries the label, not a native title=.
  return collapsed ? (
    <Tooltip content={label} side="right">
      {button}
    </Tooltip>
  ) : (
    button
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
        "transition-[width] duration-[var(--dur-3)] ease-out",
        collapsed ? "w-[68px] items-center px-3.5" : "w-[220px] px-3"
      )}
    >
      {/* Header — logo + name, with the collapse/expand control */}
      {collapsed ? (
        // Collapsed: logo centered; hovering swaps it for the expand button.
        <Tooltip content="Expand sidebar" side="right">
          <button
            onClick={toggleSidebar}
            aria-label="Expand sidebar"
            className="group relative flex h-9 w-9 items-center justify-center rounded-[var(--radius-m)] hover:bg-surface-2"
          >
            <Logo
              size={28}
              className="transition-opacity duration-[var(--dur-1)] group-hover:opacity-0"
            />
            <PanelLeftOpen
              size={18}
              strokeWidth={1.75}
              aria-hidden="true"
              className="absolute text-text-secondary opacity-0 transition-opacity duration-[var(--dur-1)] group-hover:opacity-100"
            />
          </button>
        </Tooltip>
      ) : (
        // Expanded: logo + name on the left, minimize button on the right.
        <div className="flex h-9 items-center justify-between px-1">
          <div className="flex items-center gap-2.5">
            <Logo size={26} />
            <span className="text-base font-semibold tracking-tight text-text-primary">
              OmniVox
            </span>
          </div>
          <button
            onClick={toggleSidebar}
            title="Collapse sidebar"
            aria-label="Collapse sidebar"
            className="pressable flex h-8 w-8 items-center justify-center rounded-[var(--radius-m)] text-text-muted transition-colors duration-[var(--dur-2)] ease-out hover:bg-surface-2 hover:text-text-secondary"
          >
            <PanelLeftClose size={18} strokeWidth={1.75} />
          </button>
        </div>
      )}

      {/* Separator */}
      <div className={cn("my-5 h-px bg-border", collapsed ? "w-7" : "w-full")} />

      {/* Navigation — grouped tabs (flex-1 pushes Settings to the bottom).
          Collapsed, the eyebrows have nowhere to go, so a hairline carries the
          grouping instead. */}
      <nav className={cn("flex flex-1 flex-col", collapsed ? "items-center gap-2" : "gap-4")}>
        {navGroups.map((group, i) => (
          <div
            key={group.label}
            className={cn("flex flex-col gap-0.5", collapsed && "w-full items-center gap-1")}
          >
            {collapsed
              ? i > 0 && <div className="mb-1 h-px w-7 bg-border" aria-hidden="true" />
              : <span className="eyebrow mb-1.5 px-3">{group.label}</span>}
            {group.items.map((item) => (
              <NavButton key={item.page} {...item} collapsed={collapsed} />
            ))}
          </div>
        ))}
      </nav>

      {/* Settings — pinned to the bottom, separated from the main tabs */}
      <div className={cn("my-2 h-px bg-border", collapsed ? "w-7" : "w-full")} />
      <NavButton page="settings" icon={Settings} label="Settings" collapsed={collapsed} />

      {/* Recording status dot */}
      <div className="mt-2 flex h-6 items-center justify-center">
        {isRecording && (
          <span
            className="block h-1.5 w-1.5 animate-breathe rounded-full bg-recording-500"
          />
        )}
      </div>
    </aside>
  );
}
