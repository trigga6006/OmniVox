import { useDeferredValue, useMemo, useState } from "react";
import { CalendarDays, Check, CheckCircle2, Circle, Copy, ExternalLink, Search, UserRound, X } from "lucide-react";
import { Button, Select } from "@/components/ui";
import type { Meeting, MeetingActionItem } from "@/lib/tauri";
import { cn } from "@/lib/utils";

export interface MeetingFollowUp {
  id: string | null;
  meetingId: string;
  meetingTitle: string;
  meetingStartedAt: string;
  summaryStale: boolean;
  task: string;
  owner: string | null;
  due: string | null;
  segmentRefs: string[];
  completed: boolean;
}

export type FollowUpDueState = {
  kind: "overdue" | "today" | "tomorrow" | "upcoming";
  day: number;
  label: string;
};

const DAY_MS = 86_400_000;

function localDay(date: Date) {
  return Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) / DAY_MS;
}

export function followUpDueState(due: string | null, now = new Date()): FollowUpDueState | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})(?:$|T)/.exec(due?.trim() ?? "");
  if (!match) return null;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const date = Number(match[3]);
  const parsed = new Date(year, month - 1, date);
  if (parsed.getFullYear() !== year || parsed.getMonth() !== month - 1 || parsed.getDate() !== date) return null;
  const day = localDay(parsed);
  const difference = day - localDay(now);
  if (difference < 0) return { kind: "overdue", day, label: `Overdue · ${due!.trim()}` };
  if (difference === 0) return { kind: "today", day, label: "Due today" };
  if (difference === 1) return { kind: "tomorrow", day, label: "Due tomorrow" };
  return { kind: "upcoming", day, label: `Due ${due!.trim()}` };
}

export function sortFollowUpsForInbox(items: MeetingFollowUp[], now = new Date()) {
  return [...items].sort((left, right) => {
    if (left.completed !== right.completed) return Number(left.completed) - Number(right.completed);
    if (!left.completed) {
      const leftDue = followUpDueState(left.due, now);
      const rightDue = followUpDueState(right.due, now);
      if (Boolean(leftDue) !== Boolean(rightDue)) return leftDue ? -1 : 1;
      if (leftDue && rightDue && leftDue.day !== rightDue.day) return leftDue.day - rightDue.day;
    }
    return right.meetingStartedAt.localeCompare(left.meetingStartedAt);
  });
}

export function followUpsChecklist(items: MeetingFollowUp[]) {
  return items.filter((item) => !item.completed).map((item) => {
    const metadata = [item.owner ? `Owner: ${item.owner}` : "", item.due ? `Due: ${item.due}` : "", `From: ${item.meetingTitle}`].filter(Boolean);
    return `- [ ] ${item.task}${metadata.length ? ` (${metadata.join(" · ")})` : ""}`;
  }).join("\n");
}

export function extractMeetingFollowUps(meetings: Meeting[], storedItems: MeetingActionItem[] = []): MeetingFollowUp[] {
  const items: MeetingFollowUp[] = [];
  const meetingsById = new Map(meetings.map((meeting) => [meeting.id, meeting]));
  const storedTasks = new Map<string, Set<string>>();
  for (const action of storedItems) {
    const meeting = meetingsById.get(action.meeting_id);
    if (!meeting) continue;
    const tasks = storedTasks.get(meeting.id) ?? new Set<string>();
    tasks.add(action.task.toLocaleLowerCase());
    storedTasks.set(meeting.id, tasks);
    items.push({
      id: action.id,
      meetingId: meeting.id,
      meetingTitle: meeting.title,
      meetingStartedAt: meeting.started_at,
      summaryStale: meeting.summary_stale,
      task: action.task,
      owner: action.owner,
      due: action.due,
      segmentRefs: action.segment_refs,
      completed: action.completed,
    });
  }
  for (const meeting of meetings) {
    if (meeting.actions_materialized) continue;
    if (!meeting.summary_json) continue;
    try {
      const summary = JSON.parse(meeting.summary_json) as Record<string, unknown>;
      if (!Array.isArray(summary.action_items)) continue;
      const seen = new Set<string>();
      for (const raw of summary.action_items) {
        if (!raw || typeof raw !== "object") continue;
        const action = raw as Record<string, unknown>;
        const task = typeof action.task === "string" ? action.task.trim() : "";
        if (!task || seen.has(task) || storedTasks.get(meeting.id)?.has(task.toLocaleLowerCase())) continue;
        seen.add(task);
        items.push({
          id: null,
          meetingId: meeting.id,
          meetingTitle: meeting.title,
          meetingStartedAt: meeting.started_at,
          summaryStale: meeting.summary_stale,
          task,
          owner: typeof action.owner === "string" && action.owner.trim() ? action.owner.trim() : null,
          due: typeof action.due === "string" && action.due.trim() ? action.due.trim() : null,
          segmentRefs: Array.isArray(action.segment_refs)
            ? action.segment_refs.filter((value): value is string => typeof value === "string")
            : [],
          completed: meeting.completed_actions.includes(task),
        });
      }
    } catch {
      // User-edited or legacy summaries can be plain Markdown. They remain
      // available in the meeting without poisoning the cross-meeting queue.
    }
  }
  return sortFollowUpsForInbox(items);
}

export function MeetingFollowUps({
  meetings,
  actionItems = [],
  busyKey,
  onToggle,
  onOpenMeeting,
}: {
  meetings: Meeting[];
  actionItems?: MeetingActionItem[];
  busyKey: string | null;
  onToggle: (item: MeetingFollowUp, completed: boolean) => void;
  onOpenMeeting: (meetingId: string, reference?: string) => void;
}) {
  const [showCompleted, setShowCompleted] = useState(false);
  const [query, setQuery] = useState("");
  const [owner, setOwner] = useState("__all__");
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");
  const items = useMemo(() => extractMeetingFollowUps(meetings, actionItems), [actionItems, meetings]);
  const deferredQuery = useDeferredValue(query.trim().toLocaleLowerCase());
  const pending = items.filter((item) => !item.completed);
  const completed = items.filter((item) => item.completed);
  const owners = useMemo(() => Array.from(new Set(items.flatMap((item) => item.owner?.trim() ? [item.owner.trim()] : []))).sort((left, right) => left.localeCompare(right)), [items]);
  const visible = useMemo(() => (showCompleted ? items : pending).filter((item) => {
    if (owner === "__unassigned__" && item.owner) return false;
    if (owner !== "__all__" && owner !== "__unassigned__" && item.owner !== owner) return false;
    return !deferredQuery || `${item.task} ${item.owner ?? ""} ${item.due ?? ""} ${item.meetingTitle}`.toLocaleLowerCase().includes(deferredQuery);
  }), [deferredQuery, items, owner, pending, showCompleted]);
  const filteredOpen = visible.filter((item) => !item.completed);

  const copyVisible = async () => {
    const markdown = followUpsChecklist(filteredOpen);
    if (!markdown) return;
    try {
      await navigator.clipboard.writeText(markdown);
      setCopyState("copied");
    } catch {
      setCopyState("failed");
    }
    window.setTimeout(() => setCopyState("idle"), 1400);
  };

  const ownerOptions = [
    { value: "__all__", label: "All owners" },
    { value: "__unassigned__", label: "Unassigned" },
    ...owners.map((name) => ({ value: name, label: name })),
  ];

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex shrink-0 items-center gap-2 border-b border-border px-6 py-3">
        <CheckCircle2 size={15} className="shrink-0 text-success" />
        <h2 className="shrink-0 text-sm font-semibold tracking-[-0.01em] text-text-primary">Follow-ups</h2>
        <p className="min-w-0 flex-1 truncate text-xs text-text-muted">
          Your open commitments across every meeting, including manually managed work.
        </p>
        <span className="tnum shrink-0 font-mono text-xs text-text-muted">{pending.length} open</span>
        {completed.length > 0 && (
          <Button size="sm" variant="ghost" onClick={() => setShowCompleted((value) => !value)}>
            {showCompleted ? "Hide completed" : `Show ${completed.length} completed`}
          </Button>
        )}
      </header>

      {items.length > 0 && (
        <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border px-6 py-2">
          <div className="flex h-[var(--control-h-s)] min-w-52 flex-1 items-center gap-2 rounded-[var(--radius-m)] border border-border bg-surface-2/50 px-2.5 transition-colors duration-[var(--dur-2)] ease-out focus-within:border-amber-500/40">
            <Search size={12} className="shrink-0 text-text-muted" />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              aria-label="Search follow-ups"
              placeholder="Search tasks, owners, meetings, or dates"
              className="min-w-0 flex-1 bg-transparent text-xs text-text-primary outline-none placeholder:text-text-muted/60"
            />
            {query && (
              <button
                onClick={() => setQuery("")}
                aria-label="Clear follow-up search"
                className="rounded-[var(--radius-s)] p-0.5 text-text-muted hover:text-text-primary"
              >
                <X size={11} />
              </button>
            )}
          </div>
          <div className="w-40 shrink-0">
            <Select
              aria-label="Filter follow-ups by owner"
              options={ownerOptions}
              value={owner}
              onChange={setOwner}
              className="h-[var(--control-h-s)] text-xs"
            />
          </div>
          <Button
            size="sm"
            variant="secondary"
            disabled={filteredOpen.length === 0}
            onClick={() => void copyVisible()}
            className={cn(copyState === "copied" && "text-success", copyState === "failed" && "text-recording-300")}
            icon={copyState === "copied" ? <Check /> : <Copy />}
          >
            {copyState === "copied"
              ? "Copied"
              : copyState === "failed"
                ? "Copy failed"
                : `Copy open${filteredOpen.length ? ` · ${filteredOpen.length}` : ""}`}
          </Button>
          <span className="text-xs text-text-muted">Due dates in YYYY-MM-DD are prioritized automatically</span>
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto p-5">
        {visible.length === 0 ? (
          <div className="flex min-h-72 flex-col items-center justify-center text-center">
            <div className="flex h-11 w-11 items-center justify-center rounded-full bg-surface-2 text-text-muted">
              <Check size={19} />
            </div>
            <h3 className="mt-4 text-sm font-semibold text-text-primary">
              {items.length === 0
                ? "No follow-ups yet"
                : deferredQuery || owner !== "__all__" ? "No matching follow-ups" : "Everything is complete"}
            </h3>
            <p className="mt-1 max-w-sm text-xs leading-relaxed text-text-muted">
              {items.length === 0
                ? "Generated and manually added action items will appear here with their source meeting."
                : deferredQuery || owner !== "__all__"
                  ? "Try a different search or owner filter."
                  : "Completed items stay available whenever you need the history."}
            </p>
          </div>
        ) : (
          <div className="mx-auto max-w-3xl space-y-2">
            {visible.map((item) => (
              <FollowUpRow
                key={item.id ?? `${item.meetingId}:${item.task}`}
                item={item}
                busy={busyKey === `${item.meetingId}:${item.task}`}
                onToggle={onToggle}
                onOpenMeeting={onOpenMeeting}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function FollowUpRow({
  item,
  busy,
  onToggle,
  onOpenMeeting,
}: {
  item: MeetingFollowUp;
  busy: boolean;
  onToggle: (item: MeetingFollowUp, completed: boolean) => void;
  onOpenMeeting: (meetingId: string, reference?: string) => void;
}) {
  const dueState = followUpDueState(item.due);
  return (
    <article
      className={cn(
        "group rounded-[var(--radius-l)] border p-4 transition-colors duration-[var(--dur-2)] ease-out",
        item.completed ? "border-border bg-surface-2/25" : "border-border bg-surface-2/45 hover:border-border-hover"
      )}
    >
      <div className="flex items-start gap-3">
        <button
          disabled={busy}
          onClick={() => onToggle(item, !item.completed)}
          aria-label={`${item.completed ? "Reopen" : "Complete"} ${item.task}`}
          className={cn(
            "mt-0.5 shrink-0 rounded-full transition-colors duration-[var(--dur-2)] ease-out disabled:opacity-45",
            item.completed ? "text-success" : "text-text-muted hover:text-success"
          )}
        >
          {item.completed ? <CheckCircle2 size={18} /> : <Circle size={18} />}
        </button>

        <div className="min-w-0 flex-1">
          <p className={cn("select-text text-sm leading-6", item.completed ? "text-text-muted line-through" : "text-text-primary")}>
            {item.task}
          </p>
          <div className="mt-2 flex flex-wrap items-center gap-1.5 text-xs text-text-muted">
            {item.owner && (
              <span className="flex items-center gap-1 rounded-[var(--radius-s)] bg-surface-3 px-1.5 py-0.5">
                <UserRound size={11} />
                {item.owner}
              </span>
            )}
            {item.due && (
              <span
                className={cn(
                  "flex items-center gap-1 rounded-[var(--radius-s)] px-1.5 py-0.5",
                  dueState?.kind === "overdue"
                    ? "bg-recording-500/[0.12] text-recording-300"
                    : dueState?.kind === "today"
                      ? "bg-amber-500/[0.16] text-amber-300"
                      : "bg-amber-500/[0.08] text-amber-300"
                )}
              >
                <CalendarDays size={11} />
                {dueState?.label ?? `Due ${item.due}`}
              </span>
            )}
            {item.segmentRefs.length > 0 && (
              <span className="tnum flex items-center gap-1 font-mono">
                {item.segmentRefs.slice(0, 3).map((reference) => (
                  <button
                    key={reference}
                    onClick={() => onOpenMeeting(item.meetingId, reference)}
                    aria-label={`Open ${item.meetingTitle} at ${reference.replace(/[[\]]/g, "")}`}
                    className="rounded-[var(--radius-s)] px-1 py-0.5 transition-colors duration-[var(--dur-2)] ease-out hover:bg-surface-3 hover:text-amber-300"
                  >
                    {reference.replace(/[[\]]/g, "")}
                  </button>
                ))}
              </span>
            )}
            {item.summaryStale && <span className="text-amber-300">Source changed</span>}
          </div>
        </div>

        <button
          onClick={() => onOpenMeeting(item.meetingId)}
          aria-label={`Open ${item.meetingTitle}`}
          className="flex shrink-0 items-center gap-1 rounded-[var(--radius-m)] px-2 py-1.5 text-xs text-text-muted opacity-70 transition-colors duration-[var(--dur-2)] ease-out hover:bg-surface-3 hover:text-text-primary group-hover:opacity-100"
        >
          <span className="max-w-40 truncate">{item.meetingTitle}</span>
          <ExternalLink size={11} />
        </button>
      </div>
    </article>
  );
}
