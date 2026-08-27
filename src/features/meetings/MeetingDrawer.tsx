import { useCallback, useEffect, useState } from "react";
import { ChevronDown, Settings2, X } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  closeMeetingDrawer,
  listMeetings,
  onMeetingSelect,
  onMeetingUpdated,
  type Meeting,
} from "@/lib/tauri";
import { Logo } from "@/components/Logo";
import { cn } from "@/lib/utils";
import { MeetingWorkspace } from "./MeetingWorkspace";
import { OpenRouterPanel } from "./OpenRouterPanel";
import { useMeetingRuntime } from "./useMeetingRuntime";

const ICON_BUTTON =
  "flex h-7 w-7 items-center justify-center rounded-lg text-text-muted transition-colors duration-150 hover:bg-surface-2 hover:text-text-primary";

/// The window is opened with `?meeting=<id>` when it should land on a specific
/// meeting, so the selection survives a WebView that mounts after the
/// `meeting-select` event fires.
const requestedMeetingId = () => new URLSearchParams(window.location.search).get("meeting");

export function MeetingDrawer() {
  const [meetings, setMeetings] = useState<Meeting[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(requestedMeetingId);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const { runtime } = useMeetingRuntime();

  useEffect(() => {
    document.documentElement.style.background = "transparent";
    document.documentElement.dataset.theme = "dark";
    document.body.style.background = "transparent";
    document.body.style.margin = "0";
    document.body.style.overflow = "hidden";
  }, []);

  const refresh = useCallback(() => {
    listMeetings().then((rows) => {
      setMeetings(rows);
      setSelectedId((current) => current && rows.some((row) => row.id === current) ? current : runtime.meeting_id ?? rows[0]?.id ?? null);
    }).catch(() => {});
  }, [runtime.meeting_id]);

  useEffect(() => {
    refresh();
    const unUpdated = onMeetingUpdated(refresh);
    const unSelected = onMeetingSelect((id) => { setSelectedId(id); setSettingsOpen(false); });
    return () => { unUpdated.then((fn) => fn()); unSelected.then((fn) => fn()); };
  }, [refresh]);

  const selected = meetings.find((meeting) => meeting.id === selectedId);

  return (
    <div className="flex h-screen w-screen select-none flex-col overflow-hidden rounded-2xl border border-border bg-surface-1/[0.98] text-text-primary shadow-[var(--shadow-lg)]">
      <header data-tauri-drag-region className="flex h-11 shrink-0 items-center gap-2 border-b border-border px-3">
        <div data-tauri-drag-region className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-amber-500/[0.12]">
          <Logo size={16} />
        </div>
        <button
          onMouseDown={(event) => event.stopPropagation()}
          onClick={() => setPickerOpen((open) => !open)}
          className="flex min-w-0 max-w-[230px] items-center gap-1.5 rounded-lg px-2 py-1 text-left transition-colors duration-150 hover:bg-surface-2"
        >
          <span className="truncate text-xs font-semibold">{settingsOpen ? "Meeting intelligence" : selected?.title ?? "Meeting notes"}</span>
          <ChevronDown size={12} className={cn("shrink-0 text-text-muted transition-transform duration-150", pickerOpen && "rotate-180")} />
        </button>
        <div className="ml-auto flex items-center gap-1" onMouseDown={(event) => event.stopPropagation()}>
          <button onClick={() => setSettingsOpen((open) => !open)} className={cn(ICON_BUTTON, settingsOpen && "bg-amber-500/[0.12] text-amber-300")} aria-label="Meeting provider settings"><Settings2 size={14} /></button>
          <button onClick={() => closeMeetingDrawer().catch(() => {})} className={ICON_BUTTON} aria-label="Close meeting drawer"><X size={14} /></button>
        </div>
        {pickerOpen && (
          <div className="absolute left-3 right-3 top-11 z-40 max-h-64 overflow-y-auto rounded-[var(--radius-l)] border border-border bg-surface-2 p-1.5 shadow-[var(--shadow-lg)]">
            <div className="eyebrow px-2.5 py-2">Recent meetings</div>
            {meetings.map((meeting) => (
              <button
                key={meeting.id}
                onClick={() => { setSelectedId(meeting.id); setSettingsOpen(false); setPickerOpen(false); }}
                className={cn(
                  "w-full rounded-[var(--radius-s)] px-2.5 py-2 text-left transition-colors duration-[var(--dur-2)] ease-out hover:bg-surface-3",
                  meeting.id === selectedId && "bg-amber-500/[0.12]"
                )}
              >
                <div className="truncate text-xs font-medium text-text-primary">{meeting.title}</div>
                <div className="tnum mt-0.5 text-xs text-text-muted">
                  {new Date(meeting.started_at).toLocaleString()}
                </div>
              </button>
            ))}
          </div>
        )}
      </header>
      <div className="min-h-0 flex-1">
        {settingsOpen ? (
          <div className="h-full overflow-y-auto p-3"><OpenRouterPanel compact /></div>
        ) : selectedId ? (
          <MeetingWorkspace meetingId={selectedId} compact onDeleted={refresh} />
        ) : (
          <div className="flex h-full items-center justify-center px-6 text-center text-xs text-text-muted">
            Start a meeting from OmniVox to use this drawer.
          </div>
        )}
      </div>
      <div
        aria-hidden
        onMouseDown={(event) => { event.preventDefault(); getCurrentWindow().startResizeDragging("SouthEast").catch(() => {}); }}
        className="group absolute bottom-0 right-0 flex h-4 w-4 cursor-se-resize items-end justify-end p-[3px]"
      >
        <svg width="8" height="8" viewBox="0 0 8 8" fill="none" className="text-text-muted/40 transition-colors duration-150 group-hover:text-text-muted">
          <path d="M8 1 1 8M8 5 5 8" stroke="currentColor" strokeWidth="1" strokeLinecap="round" />
        </svg>
      </div>
    </div>
  );
}
