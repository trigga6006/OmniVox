import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  AlertTriangle,
  BookmarkPlus,
  CalendarDays,
  Check,
  CircleHelp,
  CircleDollarSign,
  Clipboard,
  Copy,
  Flag,
  FileDown,
  ExternalLink,
  FolderOpen,
  History,
  Loader2,
  ListTodo,
  MessageSquareText,
  Mic,
  MoreHorizontal,
  Pause,
  Pencil,
  Play,
  Plus,
  RotateCcw,
  Search,
  Send,
  SlidersHorizontal,
  Sparkles,
  Square,
  Star,
  StickyNote,
  Tag,
  Trash2,
  UserRound,
  Users,
  X,
} from "lucide-react";
import {
  Badge,
  Button,
  Checkbox,
  ConfirmDialog,
  Progress,
  Select,
  Textarea,
} from "@/components/ui";
import {
  addMeetingActionItem,
  deleteMeetingActionItem,
  deleteMeeting,
  deleteMeetingQuestion,
  deleteMeetingMarker,
  estimateMeetingSummary,
  askMeetingQuestion,
  exportMeeting,
  getMeeting,
  getMeetingProviderSettings,
  listMeetingSummaryVersions,
  listOpenRouterModels,
  addMeetingMarker,
  meetingPause,
  meetingResume,
  meetingStop,
  onMeetingUpdated,
  openOpenRouterCredits,
  revealMeetingExport,
  saveMeetingAsTemplate,
  retryMeetingTranscription,
  restoreMeetingSummaryVersion,
  setMeetingFavorite,
  setMeetingActionItemCompleted,
  setMeetingTags,
  summarizeMeeting,
  updateMeetingNotes,
  updateMeetingAiOptions,
  updateMeetingActionItem,
  updateMeetingMetadata,
  updateMeetingMarker,
  updateMeetingSummary,
  updateMeetingSegment,
  updateMeetingSegmentSpeaker,
  updateMeetingTitle,
  type MeetingDetail,
  type MeetingActionItem,
  type MeetingExportFormat,
  type MeetingExportResult,
  type MeetingSummaryEstimate,
  type MeetingSummaryVersion,
  type MeetingProviderSettings,
  type OpenRouterModel,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { assessModelForMeeting, compactTokens } from "./modelCatalog";
import { formatMeetingDuration, useMeetingRuntime } from "./useMeetingRuntime";
import { MeetingWaveform } from "./MeetingWaveform";

type Tab = "notes" | "summary" | "actions" | "transcript" | "ask";

const SUMMARY_PRESET_OPTIONS: Array<[MeetingProviderSettings["summary_preset"], string]> = [
  ["general", "General notes"],
  ["executive", "Executive brief"],
  ["one_on_one", "1:1 conversation"],
  ["sales", "Sales call"],
  ["interview", "Interview"],
  ["standup", "Standup"],
];

type BadgeTone = "amber" | "violet" | "blue" | "red" | "green" | "neutral";

const STATUS_CHIP: Record<string, { label: string; tone: BadgeTone }> = {
  recording: { label: "Live", tone: "red" },
  paused: { label: "Paused", tone: "amber" },
  transcribing: { label: "Transcribing", tone: "violet" },
  awaiting_summary: { label: "Needs notes", tone: "amber" },
  summarizing: { label: "Writing", tone: "violet" },
  ready: { label: "Ready", tone: "green" },
  interrupted: { label: "Interrupted", tone: "amber" },
  failed: { label: "Failed", tone: "red" },
  error: { label: "Error", tone: "red" },
};

/** The backend rejects a second concurrent summarize with this message. That is
 *  not a failure from the user's point of view — the work they asked for is
 *  already happening, so the CTA should slide into its progress state. */
const ALREADY_RUNNING = /already being generated|already in progress/i;

/** Statuses where the meeting has ended but no AI notes exist yet. */
const AWAITING_NOTES = new Set(["transcribing", "awaiting_summary", "summarizing"]);

/** Clipboard write with a legacy fallback: the drawer WebView can be denied
 *  async clipboard access while it is not the focused window. */
async function copyText(value: string) {
  if (!value) return false;
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(value);
      return true;
    }
  } catch {
    // Fall through to the synchronous path below.
  }
  try {
    const area = document.createElement("textarea");
    area.value = value;
    area.setAttribute("readonly", "");
    area.style.position = "fixed";
    area.style.top = "-1000px";
    area.style.opacity = "0";
    document.body.appendChild(area);
    area.select();
    const copied = document.execCommand("copy");
    document.body.removeChild(area);
    return copied;
  } catch {
    return false;
  }
}

/** Copy control with local "Copied" feedback. Deliberately self-contained so it
 *  works in the drawer window, which has no ToastContainer. */
function CopyButton({ text, label, disabled }: { text: string; label: string; disabled?: boolean }) {
  const [state, setState] = useState<"idle" | "copied" | "failed">("idle");
  const timer = useRef<number | null>(null);
  useEffect(() => () => { if (timer.current) window.clearTimeout(timer.current); }, []);
  const run = async () => {
    const copied = await copyText(text);
    setState(copied ? "copied" : "failed");
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setState("idle"), 1600);
  };
  return (
    <Button
      size="sm"
      variant="ghost"
      aria-label={label}
      disabled={disabled || !text.trim()}
      onClick={() => void run()}
      icon={state === "copied" ? <Check /> : <Copy />}
      className={cn(state === "copied" && "text-success", state === "failed" && "text-recording-300")}
    >
      {state === "copied" ? "Copied" : state === "failed" ? "Copy failed" : label}
    </Button>
  );
}

function MetaDot() {
  return <span aria-hidden className="text-text-muted/40">·</span>;
}

const META_INPUT = "min-w-0 flex-1 bg-transparent text-xs text-text-secondary outline-none placeholder:text-text-muted/60 focus:text-text-primary";

/**
 * One row of the ⋯ actions menu. Nine copies of the same JSX lived inline
 * before; the shape is here once so the kit's menu chrome (radius-s rows on a
 * surface-2 popover, 45% disabled, no hover on disabled) stays in one place.
 */
function MenuItem({
  icon,
  children,
  detail,
  onClick,
  disabled,
  tone = "default",
}: {
  icon: ReactNode;
  children: ReactNode;
  detail?: string;
  onClick: () => void;
  disabled?: boolean;
  tone?: "default" | "danger";
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "flex w-full gap-2 rounded-[var(--radius-s)] px-2.5 py-2 text-left text-xs",
        "transition-colors duration-[var(--dur-1)] ease-out",
        "disabled:cursor-not-allowed disabled:opacity-45 disabled:hover:bg-transparent",
        detail ? "items-start" : "items-center",
        tone === "danger"
          ? "text-recording-300 hover:bg-recording-500/10 hover:text-recording-200 disabled:hover:text-recording-300"
          : "text-text-secondary hover:bg-surface-3 hover:text-text-primary disabled:hover:text-text-secondary"
      )}
    >
      <span className={cn("shrink-0 [&>svg]:size-[13px]", detail && "mt-0.5")} aria-hidden>{icon}</span>
      {detail ? (
        <span>
          <span className="block">{children}</span>
          <span className="mt-0.5 block text-xs leading-snug text-text-muted">{detail}</span>
        </span>
      ) : (
        children
      )}
    </button>
  );
}

/** The "Copy full Markdown" payload — a private archive, not the shared notes. */
function fullMarkdown(
  title: string,
  summary: string,
  actionItems: MeetingActionItem[],
  notes: string,
  transcript: Array<{ time: string; speaker: string; text: string }>,
) {
  const lines = transcript.map((s) => `[${s.time}] **${s.speaker}:** ${s.text}`).join("\n\n");
  return `# ${title}\n\n${summary}${formatManagedActions(actionItems)}\n\n## My notes\n\n${notes}\n\n## Transcript\n\n${lines}`;
}

/** Inline, always-editable metadata row in the meeting header. */
function MetaField({ icon: Icon, className, children }: { icon: typeof Tag; className?: string; children: ReactNode }) {
  return (
    <label className={cn("-ml-1.5 flex min-w-0 items-center gap-1.5 rounded-lg px-1.5 py-1 text-text-muted transition-colors hover:bg-surface-2/40 focus-within:bg-surface-2/70", className)}>
      <Icon size={12} className="shrink-0" />
      {children}
    </label>
  );
}

export function MeetingWorkspace({
  meetingId,
  compact = false,
  initialTranscriptReference,
  onInitialTranscriptReferenceConsumed,
  onDeleted,
  onOpenMeeting,
}: {
  meetingId: string;
  compact?: boolean;
  initialTranscriptReference?: string;
  onInitialTranscriptReferenceConsumed?: () => void;
  onDeleted?: () => void;
  onOpenMeeting?: (meetingId: string) => void;
}) {
  const [meeting, setMeeting] = useState<MeetingDetail | null>(null);
  const [tab, setTab] = useState<Tab>("notes");
  const [notes, setNotes] = useState("");
  const [summary, setSummary] = useState("");
  const [summaryEditing, setSummaryEditing] = useState(false);
  const [summaryHistoryOpen, setSummaryHistoryOpen] = useState(false);
  const [summaryVersions, setSummaryVersions] = useState<MeetingSummaryVersion[]>([]);
  const [summaryHistoryLoading, setSummaryHistoryLoading] = useState(false);
  const [restoringVersionId, setRestoringVersionId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [summaryEstimate, setSummaryEstimate] = useState<MeetingSummaryEstimate | null>(null);
  const [estimateLoading, setEstimateLoading] = useState(false);
  const [estimateError, setEstimateError] = useState<string | null>(null);
  const [retryBusy, setRetryBusy] = useState(false);
  const [exportBusy, setExportBusy] = useState(false);
  const [exported, setExported] = useState<MeetingExportResult | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [markerFlash, setMarkerFlash] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [transcriptQuery, setTranscriptQuery] = useState("");
  const [question, setQuestion] = useState("");
  const [askBusy, setAskBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [queued, setQueued] = useState(false);
  const [pendingRestore, setPendingRestore] = useState<MeetingSummaryVersion | null>(null);
  const [confirmTrash, setConfirmTrash] = useState(false);
  const tabTouched = useRef(false);
  const saveNotesTimer = useRef<number | null>(null);
  const saveSummaryTimer = useRef<number | null>(null);
  const loadedMeetingId = useRef<string | null>(null);
  const notesDirty = useRef(false);
  const summaryDirty = useRef(false);
  const titleDirty = useRef(false);
  const hadSummary = useRef(false);
  const latestNotes = useRef("");
  const latestSummary = useRef("");
  const estimateRequest = useRef(0);
  const { runtime } = useMeetingRuntime();

  const refresh = useCallback(() => {
    getMeeting(meetingId)
      .then((next) => {
        if (!next) return;
        const switchingMeetings = loadedMeetingId.current !== next.id;
        if (switchingMeetings) {
          loadedMeetingId.current = next.id;
          notesDirty.current = false;
          summaryDirty.current = false;
          titleDirty.current = false;
          hadSummary.current = false;
          tabTouched.current = false;
          setSummaryEditing(false);
          setSummaryHistoryOpen(false);
          setSummaryVersions([]);
          setDetailsOpen(false);
          setQueued(false);
        }
        setMeeting((current) => ({
          ...next,
          title: !switchingMeetings && titleDirty.current && current?.id === next.id
            ? current.title
            : next.title,
        }));
        if (switchingMeetings || !notesDirty.current) {
          latestNotes.current = next.user_notes;
          setNotes(next.user_notes);
        }
        if (switchingMeetings || !summaryDirty.current) {
          latestSummary.current = next.summary_markdown;
          setSummary(next.summary_markdown);
        }
        if (!hadSummary.current && next.status === "ready" && next.summary_markdown) setTab((current) => current === "notes" ? "summary" : current);
        // A meeting that just ended lands on AI notes so the generate CTA is the
        // first thing on screen — until the user picks a tab themselves.
        if (!tabTouched.current && !next.summary_markdown && AWAITING_NOTES.has(next.status)) setTab("summary");
        hadSummary.current = Boolean(next.summary_markdown);
      })
      .catch((reason) => setError(String(reason)));
  }, [meetingId]);

  useEffect(() => {
    refresh();
    const unlisten = onMeetingUpdated((id) => {
      if (id === meetingId) refresh();
    });
    return () => { void unlisten.then((fn) => fn()); };
  }, [meetingId, refresh]);

  useEffect(() => () => {
    if (saveNotesTimer.current) window.clearTimeout(saveNotesTimer.current);
    if (saveSummaryTimer.current) window.clearTimeout(saveSummaryTimer.current);
    if (notesDirty.current) void updateMeetingNotes(meetingId, latestNotes.current);
    if (summaryDirty.current) void updateMeetingSummary(meetingId, latestSummary.current);
  }, [meetingId]);

  const active = runtime.meeting_id === meetingId;
  const recording = active && runtime.status === "recording";
  const paused = active && runtime.status === "paused";
  const processing = meeting?.status === "transcribing" || meeting?.status === "summarizing";

  useEffect(() => {
    const request = ++estimateRequest.current;
    if (tab !== "summary" || active || processing || !meeting?.segments.length) {
      setSummaryEstimate(null);
      setEstimateError(null);
      setEstimateLoading(false);
      return;
    }
    setEstimateLoading(true);
    setEstimateError(null);
    estimateMeetingSummary(meetingId)
      .then((estimate) => {
        if (request === estimateRequest.current) setSummaryEstimate(estimate);
      })
      .catch((reason) => {
        if (request === estimateRequest.current) {
          setSummaryEstimate(null);
          setEstimateError(String(reason));
        }
      })
      .finally(() => {
        if (request === estimateRequest.current) setEstimateLoading(false);
      });
  }, [active, meeting?.segments.length, meeting?.updated_at, meetingId, processing, tab]);

  const changeNotes = useCallback((value: string) => {
    notesDirty.current = true;
    latestNotes.current = value;
    setNotes(value);
    if (saveNotesTimer.current) window.clearTimeout(saveNotesTimer.current);
    saveNotesTimer.current = window.setTimeout(() => {
      updateMeetingNotes(meetingId, value)
        .then(() => { if (latestNotes.current === value) notesDirty.current = false; refresh(); })
        .catch((reason) => setError(String(reason)));
    }, 450);
  }, [meetingId, refresh]);

  const changeSummary = useCallback((value: string) => {
    summaryDirty.current = true;
    latestSummary.current = value;
    setSummary(value);
    setMeeting((current) => current ? { ...current, summary_json: null } : current);
    if (saveSummaryTimer.current) window.clearTimeout(saveSummaryTimer.current);
    saveSummaryTimer.current = window.setTimeout(() => {
      updateMeetingSummary(meetingId, value)
        .then(() => { if (latestSummary.current === value) summaryDirty.current = false; })
        .catch((reason) => setError(String(reason)));
    }, 450);
  }, [meetingId]);

  const generate = useCallback(async () => {
    setQueued(false);
    if (summaryEstimate && !summaryEstimate.allowed) {
      setError(summaryEstimate.blocking_reason ?? "This summary is outside your current AI spending limits");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      if (notesDirty.current) {
        if (saveNotesTimer.current) window.clearTimeout(saveNotesTimer.current);
        await updateMeetingNotes(meetingId, latestNotes.current);
        notesDirty.current = false;
      }
      await summarizeMeeting(meetingId);
      setTab("summary");
      if (summaryHistoryOpen) {
        void listMeetingSummaryVersions(meetingId).then(setSummaryVersions).catch((reason) => setError(String(reason)));
      }
      refresh();
    }
    catch (reason) {
      // A double-click, or an auto_summarize run that beat us to it: the notes
      // are on their way, so fall through to the progress state instead of
      // shouting at the user.
      if (ALREADY_RUNNING.test(String(reason))) { setTab("summary"); refresh(); }
      else setError(String(reason));
    }
    finally { setBusy(false); }
  }, [meetingId, refresh, summaryEstimate, summaryHistoryOpen]);

  /** CTA click. Transcription still running → queue it; otherwise go now. */
  const requestNotes = useCallback(() => {
    setError(null);
    if (active || meeting?.status === "transcribing") { setQueued(true); return; }
    void generate();
  }, [active, generate, meeting?.status]);

  // Fire the queued request the moment transcription hands over.
  useEffect(() => {
    if (!queued || busy) return;
    const status = meeting?.status;
    if (!status) return;
    if (meeting?.summary_markdown || status === "summarizing") { setQueued(false); return; }
    if (status === "awaiting_summary" || status === "ready") void generate();
    else if (status === "error" || status === "failed") setQueued(false);
  }, [busy, generate, meeting?.status, meeting?.summary_markdown, queued]);

  const toggleSummaryHistory = useCallback(async () => {
    if (summaryHistoryOpen) {
      setSummaryHistoryOpen(false);
      return;
    }
    setSummaryHistoryOpen(true);
    setSummaryHistoryLoading(true);
    try {
      setSummaryVersions(await listMeetingSummaryVersions(meetingId));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setSummaryHistoryLoading(false);
    }
  }, [meetingId, summaryHistoryOpen]);

  const restoreSummaryVersion = useCallback(async (version: MeetingSummaryVersion) => {
    setPendingRestore(null);
    setRestoringVersionId(version.id);
    setError(null);
    try {
      if (summaryDirty.current) {
        if (saveSummaryTimer.current) window.clearTimeout(saveSummaryTimer.current);
        await updateMeetingSummary(meetingId, latestSummary.current);
        summaryDirty.current = false;
      }
      await restoreMeetingSummaryVersion(version.id, meetingId);
      setSummaryEditing(false);
      refresh();
      setSummaryVersions(await listMeetingSummaryVersions(meetingId));
      setNotice("Restored an earlier notes version. The version you replaced is still available.");
    } catch (reason) {
      setError(String(reason));
    } finally {
      setRestoringVersionId(null);
    }
  }, [meetingId, refresh]);

  const retryTranscription = useCallback(async () => {
    setRetryBusy(true);
    setError(null);
    try {
      await retryMeetingTranscription(meetingId);
      refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setRetryBusy(false);
    }
  }, [meetingId, refresh]);

  const markMoment = useCallback(async () => {
    setError(null);
    try {
      await addMeetingMarker(meetingId);
      setMarkerFlash(true);
      window.setTimeout(() => setMarkerFlash(false), 1400);
      refresh();
    } catch (reason) { setError(String(reason)); }
  }, [meetingId, refresh]);

  const runExport = useCallback(async (format: MeetingExportFormat) => {
    setExportBusy(true);
    setError(null);
    setMenuOpen(false);
    try {
      const result = await exportMeeting(meetingId, format);
      setExported(result);
    } catch (reason) { setError(String(reason)); }
    finally { setExportBusy(false); }
  }, [meetingId]);

  const saveAsTemplate = useCallback(async () => {
    if (!meeting) return;
    const name = window.prompt("Name this reusable meeting setup", meeting.title);
    if (name == null) return;
    setError(null);
    setMenuOpen(false);
    try {
      const template = await saveMeetingAsTemplate(meetingId, name);
      setNotice(`Saved “${template.name}” as a reusable meeting setup.`);
    } catch (reason) {
      setError(String(reason));
    }
  }, [meeting, meetingId]);

  useEffect(() => {
    if (!active) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.altKey && event.key.toLowerCase() === "m") {
        event.preventDefault();
        void markMoment();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [active, markMoment]);

  const transcript = useMemo(() => meeting?.segments.map((segment) => ({
    ...segment,
    time: formatMeetingDuration(segment.start_ms),
    speaker: segment.speaker_label?.trim() || (segment.source === "mic" ? "You" : "Meeting"),
  })) ?? [], [meeting?.segments]);
  const transcriptText = useMemo(
    () => transcript.map((segment) => `[${segment.time}] ${segment.speaker}: ${segment.text}`).join("\n"),
    [transcript],
  );
  const deferredTranscriptQuery = useDeferredValue(transcriptQuery.trim().toLocaleLowerCase());
  const visibleTranscript = useMemo(() => deferredTranscriptQuery
    ? transcript.filter((segment) => `${segment.speaker} ${segment.text}`.toLocaleLowerCase().includes(deferredTranscriptQuery))
    : transcript, [deferredTranscriptQuery, transcript]);
  const structuredSummary = useMemo(() => parseMeetingSummary(meeting?.summary_json), [meeting?.summary_json]);
  const creditError = Boolean(meeting?.error && /credit|payment required|insufficient balance/i.test(meeting.error));

  const jumpToReference = useCallback((reference: string) => {
    setTab("transcript");
    window.setTimeout(() => {
      const match = Array.from(document.querySelectorAll<HTMLElement>("[data-segment-ref]"))
        .find((element) => element.dataset.segmentRef === reference);
      match?.scrollIntoView({ behavior: "smooth", block: "center" });
      match?.focus({ preventScroll: true });
    }, 60);
  }, []);

  useEffect(() => {
    if (!initialTranscriptReference || meeting?.id !== meetingId) return;
    jumpToReference(initialTranscriptReference);
    onInitialTranscriptReferenceConsumed?.();
  }, [initialTranscriptReference, jumpToReference, meeting?.id, meetingId, onInitialTranscriptReferenceConsumed]);

  const saveActionItem = useCallback(async (
    current: MeetingActionItem | null,
    task: string,
    owner: string,
    due: string,
  ) => {
    try {
      const stored = current
        ? await updateMeetingActionItem(current.id, meetingId, task, owner, due)
        : await addMeetingActionItem(meetingId, task, owner, due);
      setMeeting((value) => {
        if (!value) return value;
        const action_items = current
          ? value.action_items.map((item) => item.id === stored.id ? stored : item)
          : [...value.action_items, stored];
        let completed_actions = value.completed_actions;
        if (current?.completed && current.task !== stored.task) {
          completed_actions = [...completed_actions.filter((item) => item !== current.task), stored.task];
        }
        return { ...value, action_items, completed_actions };
      });
      return true;
    } catch (reason) {
      setError(String(reason));
      return false;
    }
  }, [meetingId]);

  const toggleStoredAction = useCallback(async (item: MeetingActionItem, completed: boolean) => {
    setMeeting((value) => value ? {
      ...value,
      action_items: value.action_items.map((candidate) => candidate.id === item.id ? { ...candidate, completed } : candidate),
      completed_actions: completed
        ? [...value.completed_actions.filter((task) => task !== item.task), item.task]
        : value.completed_actions.filter((task) => task !== item.task),
    } : value);
    try {
      const stored = await setMeetingActionItemCompleted(item.id, meetingId, completed);
      setMeeting((value) => value ? {
        ...value,
        action_items: value.action_items.map((candidate) => candidate.id === stored.id ? stored : candidate),
      } : value);
    } catch (reason) {
      setError(String(reason));
      refresh();
    }
  }, [meetingId, refresh]);

  const removeActionItem = useCallback(async (item: MeetingActionItem) => {
    try {
      await deleteMeetingActionItem(item.id, meetingId);
      setMeeting((value) => value ? {
        ...value,
        action_items: value.action_items.filter((candidate) => candidate.id !== item.id),
        completed_actions: value.completed_actions.filter((task) => task !== item.task),
      } : value);
    } catch (reason) {
      setError(String(reason));
    }
  }, [meetingId]);

  const askQuestion = useCallback(async () => {
    const value = question.trim();
    if (!value || askBusy) return;
    setAskBusy(true);
    setError(null);
    try {
      if (notesDirty.current) {
        if (saveNotesTimer.current) window.clearTimeout(saveNotesTimer.current);
        await updateMeetingNotes(meetingId, latestNotes.current);
        notesDirty.current = false;
      }
      const answer = await askMeetingQuestion(meetingId, value);
      setMeeting((current) => current ? { ...current, questions: [...current.questions, answer] } : current);
      setQuestion("");
    } catch (reason) {
      setError(String(reason));
    } finally {
      setAskBusy(false);
    }
  }, [askBusy, meetingId, question]);

  const deleteQuestion = useCallback(async (id: string) => {
    try {
      await deleteMeetingQuestion(id, meetingId);
      setMeeting((current) => current ? { ...current, questions: current.questions.filter((exchange) => exchange.id !== id) } : current);
    } catch (reason) {
      setError(String(reason));
    }
  }, [meetingId]);

  if (!meeting) return <div className="flex h-full items-center justify-center text-text-muted">{error ?? <Loader2 className="animate-spin" />}</div>;

  /** Drives the collapsed meta row: chips when there is something to show,
   *  a single quiet "Add details" affordance when there is not. */
  const hasMeetingDetails = meeting.tags.length > 0 || meeting.agenda.trim().length > 0 || meeting.participants.length > 0;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className={cn("shrink-0 border-b border-border", compact ? "px-3 py-2.5" : "px-6 py-3")}>
        <div className="flex items-start gap-3">
          <div className="min-w-0 flex-1">
            <input
              value={meeting.title}
              aria-label="Meeting title"
              onChange={(event) => {
                titleDirty.current = true;
                setMeeting({ ...meeting, title: event.target.value });
              }}
              onBlur={() => {
                updateMeetingTitle(meetingId, meeting.title)
                  .then(() => { titleDirty.current = false; })
                  .catch((reason) => setError(String(reason)));
              }}
              className={cn(
                "-mx-1.5 w-[calc(100%+0.75rem)] rounded-[var(--radius-m)] bg-transparent px-1.5 py-0.5",
                "font-semibold tracking-[-0.015em] text-text-primary outline-none",
                "transition-colors duration-[var(--dur-2)] ease-out hover:bg-surface-2/60 focus:bg-surface-2",
                compact ? "text-sm" : "text-base"
              )}
            />
            <div className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-text-muted">
              {!active && STATUS_CHIP[meeting.status] && (
                <Badge tone={STATUS_CHIP[meeting.status].tone}>{STATUS_CHIP[meeting.status].label}</Badge>
              )}
              <span className="tnum">
                {new Date(meeting.started_at).toLocaleString(undefined, {
                  month: "short", day: "numeric", hour: "numeric", minute: "2-digit",
                })}
              </span>
              {meeting.source_app && <><MetaDot /><span className="truncate">{meeting.source_app}</span></>}
              {meeting.summary_cost != null && (
                <><MetaDot /><span className="tnum font-mono">{formatEstimateCost(meeting.summary_cost)}</span></>
              )}
              {meeting.previous_meeting_id && (
                <>
                  <MetaDot />
                  <button
                    disabled={!onOpenMeeting}
                    onClick={() => onOpenMeeting?.(meeting.previous_meeting_id!)}
                    className="flex items-center gap-1 rounded-[var(--radius-s)] transition-colors duration-[var(--dur-2)] ease-out hover:text-amber-300 disabled:cursor-default disabled:hover:text-text-muted"
                  >
                    <History size={12} /> Previous meeting
                  </button>
                </>
              )}
              {!detailsOpen && (
                <>
                  <MetaDot />
                  {/* Tags, agenda, and participants collapse into this one row —
                      chips for what exists, one quiet affordance to edit them. */}
                  <button
                    onClick={() => setDetailsOpen(true)}
                    aria-label="Edit meeting details"
                    className="-my-1 flex min-w-0 max-w-full flex-wrap items-center gap-1.5 rounded-[var(--radius-m)] px-1.5 py-1 text-left text-xs text-text-muted transition-colors duration-[var(--dur-2)] ease-out hover:bg-surface-2/50"
                  >
                    {meeting.tags.map((tag) => (
                      <span key={tag} className="max-w-40 truncate rounded-[var(--radius-s)] bg-surface-2 px-1.5 py-0.5 text-text-secondary">
                        #{tag}
                      </span>
                    ))}
                    {meeting.agenda.trim() && (
                      <span className="flex min-w-0 items-center gap-1 rounded-[var(--radius-s)] bg-surface-2 px-1.5 py-0.5 text-text-secondary">
                        <ListTodo size={11} className="shrink-0" />
                        <span className={cn("truncate", compact ? "max-w-44" : "max-w-80")}>{meeting.agenda}</span>
                      </span>
                    )}
                    {meeting.participants.length > 0 && (
                      <span className="flex min-w-0 items-center gap-1 rounded-[var(--radius-s)] bg-surface-2 px-1.5 py-0.5 text-text-secondary">
                        <Users size={11} className="shrink-0" />
                        <span className={cn("truncate", compact ? "max-w-40" : "max-w-64")}>
                          {meeting.participants.join(", ")}
                        </span>
                      </span>
                    )}
                    <span className="flex items-center gap-1 whitespace-nowrap">
                      {hasMeetingDetails ? <Pencil size={11} /> : <Plus size={11} />}
                      {hasMeetingDetails ? "Edit details" : "Add details"}
                    </span>
                  </button>
                </>
              )}
            </div>
            {detailsOpen && (
              <div className={cn("mt-1.5 grid gap-x-4 gap-y-0.5", compact ? "grid-cols-1" : "max-w-3xl grid-cols-2")}>
                <MetaField icon={Tag} className={compact ? undefined : "col-span-2"}>
                  <input
                    autoFocus
                    key={`${meeting.id}:${meeting.tags.join("|")}`}
                    defaultValue={meeting.tags.join(", ")}
                    onBlur={(event) => {
                      const tags = event.target.value.split(",");
                      void setMeetingTags(meetingId, tags).then(refresh).catch((reason) => setError(String(reason)));
                    }}
                    placeholder="Add tags, separated by commas"
                    aria-label="Meeting tags"
                    className={META_INPUT}
                  />
                </MetaField>
                <MetaField icon={ListTodo}>
                  <input
                    key={`${meeting.id}:agenda:${meeting.agenda}`}
                    defaultValue={meeting.agenda}
                    maxLength={2000}
                    onBlur={(event) => {
                      const agenda = event.target.value.trim();
                      if (agenda !== meeting.agenda) {
                        void updateMeetingMetadata(meetingId, agenda, null)
                          .then(refresh)
                          .catch((reason) => setError(String(reason)));
                      }
                    }}
                    placeholder="Add an agenda or purpose"
                    aria-label="Meeting agenda"
                    className={META_INPUT}
                  />
                </MetaField>
                <MetaField icon={Users}>
                  <input
                    key={`${meeting.id}:participants:${meeting.participants.join("|")}`}
                    defaultValue={meeting.participants.join(", ")}
                    onBlur={(event) => {
                      const participants = event.target.value.split(",");
                      if (event.target.value !== meeting.participants.join(", ")) {
                        void updateMeetingMetadata(meetingId, null, participants)
                          .then(refresh)
                          .catch((reason) => setError(String(reason)));
                      }
                    }}
                    placeholder="Participants, separated by commas"
                    aria-label="Meeting participants"
                    className={META_INPUT}
                  />
                </MetaField>
                <button
                  onClick={() => setDetailsOpen(false)}
                  className={cn(
                    "mt-1 justify-self-start rounded-[var(--radius-s)] px-1.5 py-1 text-xs font-medium text-text-muted",
                    "transition-colors duration-[var(--dur-2)] ease-out hover:bg-surface-2 hover:text-text-primary",
                    compact ? undefined : "col-span-2"
                  )}
                >
                  Done
                </button>
              </div>
            )}
          </div>
          {(recording || paused) ? (
            <div className="flex shrink-0 items-center gap-1.5">
              <Button size="sm" variant="ghost" className="w-8 px-0" aria-label={paused ? "Resume meeting" : "Pause meeting"} onClick={() => (paused ? meetingResume() : meetingPause()).catch((reason) => setError(String(reason)))} icon={paused ? <Play /> : <Pause />} />
              <Button size="sm" variant="secondary" className="border-recording-500/25 bg-recording-500/[0.12] text-recording-300 hover:bg-recording-500/20 hover:text-recording-200 [&>svg]:size-[10px]" onClick={() => meetingStop().catch((reason) => setError(String(reason)))} icon={<Square fill="currentColor" />}>End</Button>
            </div>
          ) : (
            <div className="relative shrink-0">
              <Button
                size="sm"
                variant="ghost"
                className="w-8 px-0 [&>svg]:size-[17px]"
                onClick={() => setMenuOpen((open) => !open)}
                aria-label="Meeting actions"
                icon={<MoreHorizontal />}
              />
              {menuOpen && (
                <div className="absolute right-0 top-10 z-20 w-64 rounded-[var(--radius-l)] border border-border-hover bg-surface-2 p-1.5 shadow-[var(--shadow-lg)]">
                  <MenuItem
                    icon={<Star fill={meeting.is_favorite ? "currentColor" : "none"} />}
                    onClick={() => {
                      void setMeetingFavorite(meetingId, !meeting.is_favorite)
                        .then(() => { setMenuOpen(false); refresh(); })
                        .catch((reason) => setError(String(reason)));
                    }}
                  >
                    {meeting.is_favorite ? "Remove favorite" : "Add to favorites"}
                  </MenuItem>
                  <MenuItem icon={<BookmarkPlus />} onClick={() => void saveAsTemplate()}>
                    Save as reusable setup
                  </MenuItem>

                  <div className="eyebrow mb-1 mt-2 px-2.5">Share</div>
                  <MenuItem
                    icon={<Clipboard />}
                    disabled={!summary.trim()}
                    detail="Summary only"
                    onClick={() => { void copyText(summary); setMenuOpen(false); }}
                  >
                    Copy AI notes
                  </MenuItem>
                  <MenuItem
                    icon={<FileDown />}
                    disabled={exportBusy}
                    detail="No transcript, scratchpad, Q&A, or cost data"
                    onClick={() => void runExport("notes")}
                  >
                    Export polished notes
                  </MenuItem>

                  <div className="mx-2 my-1.5 border-t border-border" />
                  <div className="eyebrow mb-1 px-2.5">Private archive</div>
                  <MenuItem
                    icon={<FileDown />}
                    onClick={() => {
                      void copyText(fullMarkdown(meeting.title, summary, meeting.action_items, notes, transcript));
                      setMenuOpen(false);
                    }}
                  >
                    Copy full Markdown
                  </MenuItem>
                  <MenuItem icon={<FileDown />} disabled={exportBusy} onClick={() => void runExport("markdown")}>
                    Full archive + versions (.md)
                  </MenuItem>
                  <MenuItem icon={<FileDown />} disabled={exportBusy} onClick={() => void runExport("json")}>
                    Full archive + versions (.json)
                  </MenuItem>
                  <MenuItem
                    icon={<FileDown />}
                    disabled={exportBusy || transcript.length === 0}
                    onClick={() => void runExport("webvtt")}
                  >
                    Transcript captions (.vtt)
                  </MenuItem>
                  <MenuItem
                    icon={<FileDown />}
                    disabled={exportBusy || transcript.length === 0}
                    onClick={() => void runExport("srt")}
                  >
                    Transcript captions (.srt)
                  </MenuItem>

                  <div className="mx-2 my-1.5 border-t border-border" />
                  <MenuItem icon={<Trash2 />} tone="danger" onClick={() => { setMenuOpen(false); setConfirmTrash(true); }}>
                    Move to Trash
                  </MenuItem>
                </div>
              )}
            </div>
          )}
        </div>

        {active && (
          <div className="mt-3 flex flex-wrap items-center gap-x-3 gap-y-2 rounded-[var(--radius-l)] border border-recording-500/20 bg-recording-500/[0.06] px-3 py-2">
            <span className={cn("h-2 w-2 shrink-0 rounded-full", recording ? "animate-pulse bg-recording-400" : "bg-amber-400")} />
            <span className="tnum font-mono text-sm font-semibold text-text-primary">
              {formatMeetingDuration(runtime.elapsed_ms)}
            </span>
            <span className="text-xs text-text-muted">{paused ? "Paused" : "Recording locally"}</span>
            <CaptureSourceBadge label="Mic" detected={runtime.mic_signal_detected} paused={paused} />
            <CaptureSourceBadge label="Meeting" detected={runtime.system_signal_detected} paused={paused} />
            <div className="ml-auto flex items-center gap-2">
              <Button
                size="sm"
                variant={markerFlash ? "primary" : "secondary"}
                onClick={() => void markMoment()}
                title="Mark this moment (Alt+M)"
                icon={<Flag fill={markerFlash ? "currentColor" : "none"} />}
              >
                {markerFlash ? "Marked" : "Mark moment"}
              </Button>
              <MeetingWaveform level={Math.max(runtime.mic_level, runtime.system_level)} active={recording} compact />
            </div>
          </div>
        )}
        {active && runtime.capture_warning && (
          <div
            role="alert"
            className="mt-2 rounded-[var(--radius-l)] border border-amber-500/20 bg-amber-500/[0.08] px-3 py-2 text-xs leading-relaxed text-amber-100 light:text-amber-800"
          >
            {runtime.capture_warning}
          </div>
        )}
        {/* The AI-notes call to action carries its own progress bar; don't print it twice. */}
        {meeting.status === "transcribing" && !(tab === "summary" && !summary) && (
          <TranscriptionProgress progress={meeting.transcription} />
        )}
        {meeting.error && (
          <div className="mt-3 flex flex-wrap items-center gap-2 rounded-[var(--radius-l)] border border-amber-500/20 bg-amber-500/[0.07] px-3 py-2 text-xs leading-relaxed text-amber-200">
            <AlertTriangle size={13} className="shrink-0" />
            <span className="min-w-0 flex-1">{meeting.error}</span>
            {creditError && (
              <Button
                size="sm"
                variant="secondary"
                onClick={() => void openOpenRouterCredits().catch((reason) => setError(String(reason)))}
                icon={<ExternalLink />}
              >
                Add credits
              </Button>
            )}
            {meeting.transcription.failed > 0 && (
              <Button
                size="sm"
                variant="secondary"
                disabled={retryBusy || active}
                onClick={retryTranscription}
                icon={retryBusy ? <Loader2 className="animate-spin" /> : <RotateCcw />}
              >
                {retryBusy ? "Retrying" : `Retry ${meeting.transcription.failed} failed`}
              </Button>
            )}
          </div>
        )}
      </header>

      <nav className={cn("flex shrink-0 items-center border-b border-border", compact ? "px-2" : "px-4")}>
        <div className="scrollbar-none flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto">
          {([
            ["notes", StickyNote, "My notes", null],
            ["summary", Sparkles, "AI notes", null],
            ["actions", ListTodo, "Actions", meeting.action_items.filter((item) => !item.completed).length],
            ["transcript", MessageSquareText, "Transcript", transcript.length],
            ["ask", CircleHelp, "Ask", meeting.questions.length],
          ] as const).map(([id, Icon, label, count]) => {
            const on = tab === id;
            return (
              <button
                key={id}
                onClick={() => { tabTouched.current = true; setTab(id); }}
                aria-pressed={on}
                className={cn(
                  "relative flex shrink-0 items-center gap-1.5 rounded-t-[var(--radius-m)] px-2.5 py-2.5 text-xs font-medium",
                  "transition-colors duration-[var(--dur-2)] ease-out",
                  on ? "text-text-primary" : "text-text-muted hover:bg-surface-2/50 hover:text-text-secondary"
                )}
              >
                {on && <span className="absolute inset-x-1.5 -bottom-px h-0.5 rounded-full bg-amber-500" />}
                <Icon size={13} className={on ? "text-amber-400" : undefined} />
                {label}
                {count ? (
                  <span
                    className={cn(
                      "tnum rounded-[var(--radius-s)] px-1 font-mono text-xs",
                      on ? "bg-amber-500/[0.14] text-amber-300" : "bg-surface-2 text-text-muted"
                    )}
                  >
                    {count}
                  </span>
                ) : null}
              </button>
            );
          })}
        </div>
        {tab === "summary" && (
          <MeetingAiOptions
            meeting={meeting}
            disabled={busy || processing}
            onSaved={refresh}
            onError={(reason) => setError(String(reason))}
          />
        )}
      </nav>

      <main className="min-h-0 flex-1 overflow-y-auto">
        {tab === "notes" && (
          <div className={cn("mx-auto h-full w-full max-w-3xl", compact ? "p-4" : "p-6")}>
            <textarea
              autoFocus={active}
              value={notes}
              onChange={(event) => changeNotes(event.target.value)}
              placeholder="Type rough notes while the meeting runs…&#10;&#10;OmniVox will use these as signals of what matters when it creates the final notes."
              className="select-text h-full min-h-64 w-full resize-none bg-transparent text-sm leading-7 text-text-primary outline-none placeholder:text-text-muted/55"
            />
          </div>
        )}

        {tab === "summary" && (summary ? (
          <div className="flex h-full min-h-72 flex-col">
            {/* One flat control row, not a floating toolbar: meta left, quiet actions right. */}
            <div
              className={cn(
                "flex shrink-0 flex-wrap items-center gap-x-1.5 gap-y-1 border-b border-border py-1.5",
                compact ? "pl-3 pr-1.5" : "pl-6 pr-3"
              )}
            >
              <div
                className={cn(
                  "flex min-w-0 items-center gap-1.5 font-mono text-xs text-text-muted",
                  compact ? "basis-full" : "flex-1"
                )}
              >
                {meeting.summary_model && (
                  <span className="truncate" title={meeting.summary_model}>{meeting.summary_model}</span>
                )}
                {meeting.summary_model && (estimateLoading || summaryEstimate) && <MetaDot />}
                <SummaryEstimateBadge estimate={summaryEstimate} loading={estimateLoading} />
              </div>
              <CopyButton text={summary} label="Copy" />
              <Button
                size="sm"
                variant="ghost"
                className={cn(summaryHistoryOpen && "bg-surface-2 text-amber-300")}
                onClick={() => void toggleSummaryHistory()}
                icon={<History />}
              >
                Versions{summaryVersions.length > 0 ? ` · ${summaryVersions.length}` : ""}
              </Button>
              {structuredSummary && (
                <Button size="sm" variant="ghost" onClick={() => setSummaryEditing((value) => !value)}>
                  {summaryEditing ? "Read notes" : "Edit source"}
                </Button>
              )}
              <Button
                size="sm"
                variant="ghost"
                disabled={busy || processing || active || summaryEstimate?.allowed === false}
                onClick={generate}
                icon={busy ? <Loader2 className="animate-spin" /> : <RotateCcw />}
              >
                {busy ? "Updating" : "Regenerate"}
              </Button>
            </div>
            {meeting.summary_stale && (
              <p
                className={cn(
                  "shrink-0 border-b border-amber-500/20 bg-amber-500/[0.06] py-1.5 text-xs leading-relaxed text-amber-200",
                  compact ? "px-3" : "px-6"
                )}
              >
                These notes may not reflect the latest source material or were restored from history.
              </p>
            )}
            <div className={cn("flex min-h-0 flex-1", compact ? "flex-col gap-3 p-3" : "gap-6 p-6")}>
              <div className="mx-auto flex w-full min-w-0 max-w-3xl flex-1 flex-col">
                {structuredSummary && !summaryEditing ? (
                  <StructuredSummary
                    summary={structuredSummary}
                    actionItems={meeting.action_items}
                    onToggleAction={toggleStoredAction}
                    onReference={jumpToReference}
                  />
                ) : (
                  <textarea
                    value={summary}
                    onChange={(event) => changeSummary(event.target.value)}
                    aria-label="AI notes source"
                    className="select-text h-full min-h-72 w-full resize-none bg-transparent font-sans text-sm leading-7 text-text-primary outline-none"
                  />
                )}
              </div>
              {summaryHistoryOpen && (
                <SummaryHistoryPanel
                  versions={summaryVersions}
                  currentMarkdown={summary}
                  loading={summaryHistoryLoading}
                  restoringId={restoringVersionId}
                  compact={compact}
                  onRestore={setPendingRestore}
                  onClose={() => setSummaryHistoryOpen(false)}
                />
              )}
            </div>
          </div>
        ) : (
          <div className={cn("h-full", compact ? "p-3" : "p-6")}>
            <AiNotesPanel
              status={meeting.status}
              recording={active}
              busy={busy}
              queued={queued}
              transcription={meeting.transcription}
              modelName={summaryEstimate?.model_name ?? meeting.summary_model}
              estimate={summaryEstimate}
              estimateLoading={estimateLoading}
              estimateError={estimateError}
              error={meeting.error}
              onGenerate={requestNotes}
              onCancelQueue={() => setQueued(false)}
            />
          </div>
        ))}

        {tab === "actions" && (
          <ActionItemsPanel
            items={meeting.action_items}
            compact={compact}
            onSave={saveActionItem}
            onToggle={toggleStoredAction}
            onDelete={removeActionItem}
            onReference={jumpToReference}
          />
        )}

        {tab === "transcript" && (
          <div className={cn("mx-auto w-full max-w-4xl space-y-1", compact ? "p-3" : "p-5")}>
            {transcript.length > 0 && (
              <div className="mb-3 flex items-center gap-1.5 rounded-[var(--radius-l)] border border-border bg-surface-2/40 px-2.5">
                <Search size={13} className="shrink-0 text-text-muted" />
                <input
                  value={transcriptQuery}
                  onChange={(event) => setTranscriptQuery(event.target.value)}
                  placeholder="Find in transcript"
                  aria-label="Find in transcript"
                  className="h-9 min-w-0 flex-1 bg-transparent text-xs text-text-primary outline-none placeholder:text-text-muted/60"
                />
                {transcriptQuery && (
                  <>
                    <span className="tnum shrink-0 font-mono text-xs text-text-muted">
                      {visibleTranscript.length} match{visibleTranscript.length === 1 ? "" : "es"}
                    </span>
                    <button
                      onClick={() => setTranscriptQuery("")}
                      aria-label="Clear transcript search"
                      className="rounded-[var(--radius-s)] p-1 text-text-muted hover:bg-surface-3 hover:text-text-primary"
                    >
                      <X size={12} />
                    </button>
                  </>
                )}
                <CopyButton text={transcriptText} label={compact ? "Copy" : "Copy transcript"} />
              </div>
            )}
            {meeting.markers.length > 0 && (
              <section className="mb-4 rounded-[var(--radius-l)] border border-amber-500/15 bg-amber-500/[0.045] p-2">
                <div className="eyebrow flex items-center gap-1.5 px-1 pb-1.5 text-amber-300">
                  <Flag size={11} /> Important moments
                </div>
                {meeting.markers.map((marker) => (
                  <div
                    key={marker.id}
                    className="group flex items-center gap-2 rounded-[var(--radius-m)] px-1.5 py-1 hover:bg-surface-2/60"
                  >
                    <span className="tnum w-11 shrink-0 font-mono text-xs text-text-muted">
                      {formatMeetingDuration(marker.at_ms)}
                    </span>
                    <input
                      defaultValue={marker.label}
                      aria-label={`Marker at ${formatMeetingDuration(marker.at_ms)}`}
                      onBlur={(event) => {
                        const label = event.target.value.trim();
                        if (label !== marker.label) {
                          void updateMeetingMarker(marker.id, meetingId, label)
                            .then(refresh)
                            .catch((reason) => setError(String(reason)));
                        }
                      }}
                      className="min-w-0 flex-1 bg-transparent text-xs text-text-secondary outline-none focus:text-text-primary"
                    />
                    <button
                      onClick={() => {
                        void deleteMeetingMarker(marker.id, meetingId)
                          .then(refresh)
                          .catch((reason) => setError(String(reason)));
                      }}
                      aria-label={`Delete marker at ${formatMeetingDuration(marker.at_ms)}`}
                      className="rounded-[var(--radius-s)] p-1 text-text-muted opacity-0 hover:text-recording-300 group-hover:opacity-100 focus:opacity-100"
                    >
                      <Trash2 size={12} />
                    </button>
                  </div>
                ))}
              </section>
            )}
            {transcript.length === 0 ? (
              <div className="flex min-h-52 flex-col items-center justify-center text-center">
                <Loader2 size={18} className={cn("mb-3 text-text-muted", (processing || active) && "animate-spin")} />
                <p className="text-sm text-text-secondary">
                  {active ? "Listening for the first spoken segment…" : "No transcript segments yet."}
                </p>
                <p className="mt-1 max-w-xs text-xs text-text-muted">
                  Finalized chunks appear here as Whisper finishes them.
                </p>
              </div>
            ) : visibleTranscript.length === 0 ? (
              <div className="flex min-h-40 items-center justify-center text-center text-xs text-text-muted">
                No transcript lines match “{transcriptQuery.trim()}”.
              </div>
            ) : (
              visibleTranscript.map((segment) => (
                <TranscriptRow
                  key={segment.id}
                  segment={segment}
                  meetingId={meetingId}
                  compact={compact}
                  highlight={deferredTranscriptQuery}
                  onSaved={refresh}
                  onError={(reason) => setError(String(reason))}
                />
              ))
            )}
          </div>
        )}

        {tab === "ask" && (
          <QuestionPanel
            exchanges={meeting.questions}
            question={question}
            busy={askBusy}
            compact={compact}
            unavailable={active || processing || transcript.length === 0}
            onQuestion={setQuestion}
            onAsk={askQuestion}
            onDelete={deleteQuestion}
            onReference={jumpToReference}
          />
        )}
      </main>

      {exported && (
        <FootBanner tone="success" onDismiss={() => setExported(null)} dismissLabel="Dismiss export confirmation">
          <span className="min-w-0 flex-1 truncate">Saved {exported.file_name}</span>
          <button
            onClick={() => void revealMeetingExport(exported.path).catch((reason) => setError(String(reason)))}
            className="flex shrink-0 items-center gap-1 rounded-[var(--radius-s)] px-2 py-1 font-medium hover:bg-success/10"
          >
            <FolderOpen size={12} /> Show in folder
          </button>
        </FootBanner>
      )}
      {notice && (
        <FootBanner tone="success" onDismiss={() => setNotice(null)} dismissLabel="Dismiss meeting notice">
          <span className="min-w-0 flex-1">{notice}</span>
        </FootBanner>
      )}
      {error && (
        <FootBanner tone="warn">
          <span className="min-w-0 flex-1">{error}</span>
          {/credit|payment required|insufficient balance/i.test(error) && (
            <button
              onClick={() => void openOpenRouterCredits().catch((reason) => setError(String(reason)))}
              className="flex shrink-0 items-center gap-1 rounded-[var(--radius-s)] px-2 py-1 font-medium hover:bg-amber-500/10"
            >
              <ExternalLink size={11} /> Add credits
            </button>
          )}
        </FootBanner>
      )}

      <ConfirmDialog
        open={pendingRestore != null}
        title="Restore this notes version"
        description="Your current notes are saved to version history first, so nothing is lost."
        confirmLabel="Restore version"
        onConfirm={() => { if (pendingRestore) void restoreSummaryVersion(pendingRestore); }}
        onCancel={() => setPendingRestore(null)}
      />

      <ConfirmDialog
        open={confirmTrash}
        tone="danger"
        title="Move this meeting to Trash"
        description="The transcript, notes, and markers stay recoverable from Trash until you delete them permanently."
        confirmLabel="Move to trash"
        onConfirm={() => {
          setConfirmTrash(false);
          void deleteMeeting(meetingId)
            .then(() => onDeleted?.())
            .catch((reason) => setError(String(reason)));
        }}
        onCancel={() => setConfirmTrash(false)}
      />
    </div>
  );
}

/** The three status strips pinned under the workspace: export, notice, error. */
function FootBanner({
  tone,
  children,
  onDismiss,
  dismissLabel,
}: {
  tone: "success" | "warn";
  children: ReactNode;
  onDismiss?: () => void;
  dismissLabel?: string;
}) {
  return (
    <div
      className={cn(
        "flex shrink-0 items-center gap-2 border-t px-4 py-2 text-xs",
        tone === "success"
          ? "border-success/20 bg-success/[0.07] text-success"
          : "border-amber-500/20 bg-amber-500/[0.07] text-amber-200"
      )}
    >
      {children}
      {onDismiss && (
        <button
          onClick={onDismiss}
          aria-label={dismissLabel}
          className="rounded-[var(--radius-s)] px-1 opacity-70 hover:opacity-100"
        >
          ×
        </button>
      )}
    </div>
  );
}

function MeetingAiOptions({ meeting, disabled, onSaved, onError }: { meeting: MeetingDetail; disabled: boolean; onSaved: () => void; onError: (reason: unknown) => void }) {
  const [open, setOpen] = useState(false);
  const [settings, setSettings] = useState<MeetingProviderSettings | null>(null);
  const [models, setModels] = useState<OpenRouterModel[]>([]);
  const [model, setModel] = useState(meeting.ai_model_override ?? "");
  const [preset, setPreset] = useState<MeetingProviderSettings["summary_preset"] | "">(meeting.summary_preset_override ?? "");
  const [instructions, setInstructions] = useState(meeting.summary_instructions);
  const [saving, setSaving] = useState(false);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (open) return;
    setModel(meeting.ai_model_override ?? "");
    setPreset(meeting.summary_preset_override ?? "");
    setInstructions(meeting.summary_instructions);
  }, [meeting.ai_model_override, meeting.summary_instructions, meeting.summary_preset_override, open]);

  const show = useCallback(() => {
    setOpen((current) => {
      if (current) return false;
      setLoading(true);
      Promise.allSettled([getMeetingProviderSettings(), listOpenRouterModels()])
        .then(([settingsResult, modelsResult]) => {
          if (settingsResult.status === "fulfilled") setSettings(settingsResult.value);
          else onError(settingsResult.reason);
          if (modelsResult.status === "fulfilled") setModels(modelsResult.value);
        })
        .finally(() => setLoading(false));
      return true;
    });
  }, [onError]);

  const save = useCallback(async (reset = false) => {
    setSaving(true);
    try {
      await updateMeetingAiOptions(
        meeting.id,
        reset ? null : model.trim() || null,
        reset ? null : preset || null,
        reset ? "" : instructions,
      );
      if (reset) { setModel(""); setPreset(""); setInstructions(""); }
      setOpen(false);
      onSaved();
    } catch (reason) { onError(reason); }
    finally { setSaving(false); }
  }, [instructions, meeting.id, model, onError, onSaved, preset]);

  const hasOverride = Boolean(meeting.ai_model_override || meeting.summary_preset_override || meeting.summary_instructions.trim());
  const effectiveModelId = model.trim() || settings?.model || "";
  const selectedModel = models.find((candidate) => candidate.id === effectiveModelId);
  const recordedMinutes = meeting.ended_at
    ? Math.max(5, Math.ceil((Date.parse(meeting.ended_at) - Date.parse(meeting.started_at)) / 60_000))
    : 60;
  const modelAssessment = selectedModel && settings
    ? assessModelForMeeting(selectedModel, settings, null, recordedMinutes)
    : null;
  const presetOptions = [
    {
      value: "",
      label: settings
        ? `Global · ${SUMMARY_PRESET_OPTIONS.find(([value]) => value === settings.summary_preset)?.[1] ?? settings.summary_preset}`
        : "Use global default",
    },
    ...SUMMARY_PRESET_OPTIONS.map(([value, label]) => ({ value, label })),
  ];

  // Not a content tab: an icon-only control, separated from the tab strip by a
  // hairline so it never reads as a sixth destination.
  return (
    <div className="relative ml-1.5 shrink-0 border-l border-border pl-1.5">
      <Button
        size="sm"
        variant="ghost"
        disabled={disabled}
        onClick={show}
        aria-label="Meeting AI settings"
        title={hasOverride ? "Meeting AI settings · customized for this meeting" : "Meeting AI settings"}
        className={cn("w-8 px-0", hasOverride && "text-amber-300")}
        icon={<SlidersHorizontal />}
      />
      {open && (
        <div className="absolute right-0 top-10 z-40 w-80 rounded-[var(--radius-l)] border border-border-hover bg-surface-1 p-4 shadow-[var(--shadow-lg)]">
          <div className="flex items-start gap-2">
            <div className="min-w-0 flex-1">
              <h3 className="text-xs font-semibold text-text-primary">This meeting’s AI</h3>
              <p className="mt-1 text-xs leading-relaxed text-text-muted">
                Overrides apply to automatic notes, regeneration, and meeting questions. Empty fields
                inherit global defaults.
              </p>
            </div>
            {loading && <Loader2 size={13} className="animate-spin text-text-muted" />}
          </div>

          <label className="eyebrow mt-4 block">
            OpenRouter model
            <input
              list={`meeting-models-${meeting.id}`}
              value={model}
              onChange={(event) => setModel(event.target.value)}
              placeholder={settings ? `Global · ${settings.model}` : "Use global default"}
              className="mt-1.5 h-[var(--control-h-s)] w-full rounded-[var(--radius-m)] border border-border bg-surface-2 px-2 font-mono text-xs normal-case tracking-normal text-text-primary outline-none focus:border-amber-500/40"
            />
            <datalist id={`meeting-models-${meeting.id}`}>
              {models.map((candidate) => (
                <option key={candidate.id} value={candidate.id}>{candidate.name}</option>
              ))}
            </datalist>
          </label>

          {modelAssessment && selectedModel && (
            <div
              className={cn(
                "mt-2 rounded-[var(--radius-m)] border px-2.5 py-2 text-xs leading-relaxed",
                modelAssessment.fits
                  ? "border-success/15 bg-success/[0.05] text-text-muted"
                  : "border-amber-500/20 bg-amber-500/[0.07] text-amber-200"
              )}
            >
              <div className="flex items-center justify-between gap-2">
                <span className="truncate font-medium text-text-secondary">{selectedModel.name}</span>
                <span className="tnum shrink-0 font-mono">
                  ≈ {formatEstimateCost(modelAssessment.estimatedCost)} · {compactTokens(selectedModel.context_length)} ctx
                </span>
              </div>
              <p className="mt-0.5">
                {modelAssessment.fits
                  ? `Fits global guardrails for this ${recordedMinutes}-minute meeting.`
                  : modelAssessment.reasons.join(" · ")}
              </p>
            </div>
          )}
          {!loading && model.trim() && models.length > 0 && !selectedModel && (
            <p className="mt-2 text-xs leading-relaxed text-text-muted">
              Custom model slug. OmniVox will verify availability, structured output, price, context, and
              your budgets before spending.
            </p>
          )}

          <div className="mt-4">
            <span className="eyebrow block">Summary style</span>
            <div className="mt-1.5">
              <Select
                aria-label="Summary style"
                options={presetOptions}
                value={preset}
                onChange={(value) => setPreset(value as MeetingProviderSettings["summary_preset"] | "")}
                className="h-[var(--control-h-s)] text-xs"
              />
            </div>
          </div>

          <label className="eyebrow mt-4 block">
            Meeting-specific instructions
            <Textarea
              value={instructions}
              onChange={(event) => setInstructions(event.target.value)}
              maxLength={2000}
              rows={3}
              aria-label="Meeting-specific instructions"
              placeholder={
                settings?.custom_instructions
                  ? `Global: ${settings.custom_instructions}`
                  : "Example: Focus on customer objections and next steps."
              }
              className="mt-1.5 text-xs normal-case leading-5 tracking-normal"
            />
          </label>

          <div className="mt-4 flex items-center gap-2">
            {hasOverride && (
              <Button size="sm" variant="ghost" disabled={saving} onClick={() => void save(true)}>Reset</Button>
            )}
            <Button size="sm" variant="secondary" className="ml-auto" onClick={() => setOpen(false)}>Cancel</Button>
            <Button size="sm" variant="primary" disabled={saving} onClick={() => void save()}>
              {saving ? "Saving…" : "Save"}
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

type DisplaySegment = MeetingDetail["segments"][number] & { time: string; speaker: string };

function TranscriptRow({ segment, meetingId, compact, highlight, onSaved, onError }: { segment: DisplaySegment; meetingId: string; compact?: boolean; highlight?: string; onSaved: () => void; onError: (reason: unknown) => void }) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(segment.text);
  const [saving, setSaving] = useState(false);
  const [speakerEditing, setSpeakerEditing] = useState(false);
  const [speakerValue, setSpeakerValue] = useState(segment.speaker_label ?? "");
  const [speakerSaving, setSpeakerSaving] = useState(false);

  useEffect(() => {
    if (!editing) setValue(segment.text);
  }, [editing, segment.text]);

  useEffect(() => {
    if (!speakerEditing) setSpeakerValue(segment.speaker_label ?? "");
  }, [segment.speaker_label, speakerEditing]);

  const save = useCallback(async () => {
    const text = value.trim();
    if (!text || text === segment.text) { setValue(segment.text); setEditing(false); return; }
    setSaving(true);
    try {
      await updateMeetingSegment(segment.id, meetingId, text);
      setEditing(false);
      onSaved();
    } catch (reason) { onError(reason); }
    finally { setSaving(false); }
  }, [meetingId, onError, onSaved, segment.id, segment.text, value]);

  const saveSpeaker = useCallback(async (applyToSource: boolean) => {
    const label = speakerValue.trim();
    if (!applyToSource && label === (segment.speaker_label ?? "")) { setSpeakerEditing(false); return; }
    setSpeakerSaving(true);
    try {
      await updateMeetingSegmentSpeaker(segment.id, meetingId, label, applyToSource);
      setSpeakerEditing(false);
      onSaved();
    } catch (reason) { onError(reason); }
    finally { setSpeakerSaving(false); }
  }, [meetingId, onError, onSaved, segment.id, segment.speaker_label, speakerValue]);

  const resetSpeaker = useCallback(async () => {
    setSpeakerSaving(true);
    try {
      await updateMeetingSegmentSpeaker(segment.id, meetingId, "", false);
      setSpeakerValue("");
      setSpeakerEditing(false);
      onSaved();
    } catch (reason) { onError(reason); }
    finally { setSpeakerSaving(false); }
  }, [meetingId, onError, onSaved, segment.id]);

  const defaultSpeaker = segment.source === "mic" ? "You" : "Meeting";
  const speaker = (
    <SpeakerControl
      segment={segment}
      defaultSpeaker={defaultSpeaker}
      editing={speakerEditing}
      value={speakerValue}
      saving={speakerSaving}
      onToggle={() => setSpeakerEditing((current) => !current)}
      onValue={setSpeakerValue}
      onSave={saveSpeaker}
      onReset={resetSpeaker}
    />
  );

  return (
    <div
      tabIndex={-1}
      data-segment-ref={formatSegmentReference(segment.start_ms)}
      className={cn(
        "group grid gap-x-2.5 gap-y-1 rounded-[var(--radius-m)] px-2 py-2 outline-none",
        "transition-colors duration-[var(--dur-2)] ease-out hover:bg-surface-2/50 focus:bg-amber-500/[0.08]",
        compact ? "grid-cols-[1fr_24px]" : "grid-cols-[46px_84px_1fr_24px]"
      )}
    >
      {compact ? (
        <div className="col-span-2 flex min-w-0 items-center gap-2">
          <span className="tnum shrink-0 font-mono text-xs text-text-muted">{segment.time}</span>
          {speaker}
        </div>
      ) : (
        <>
          <span className="tnum pt-0.5 font-mono text-xs text-text-muted">{segment.time}</span>
          {speaker}
        </>
      )}

      {editing ? (
        <Textarea
          autoFocus
          value={value}
          onChange={(event) => setValue(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Escape") { setValue(segment.text); setEditing(false); }
            if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) void save();
          }}
          className="min-h-16 border-amber-500/30 bg-surface-0 px-2 py-1.5 leading-5"
        />
      ) : (
        <p className="select-text text-sm leading-relaxed text-text-secondary">
          <HighlightedText text={segment.text} query={highlight} />
        </p>
      )}

      {editing ? (
        <button
          disabled={saving || !value.trim()}
          onClick={() => void save()}
          aria-label={`Save transcript correction at ${segment.time}`}
          className="self-start rounded-[var(--radius-s)] p-1 text-success hover:bg-success/10 disabled:opacity-45"
        >
          {saving ? <Loader2 size={12} className="animate-spin" /> : <Check size={12} />}
        </button>
      ) : (
        <button
          onClick={() => setEditing(true)}
          aria-label={`Correct transcript at ${segment.time}`}
          className="self-start rounded-[var(--radius-s)] p-1 text-text-muted opacity-0 hover:bg-surface-3 hover:text-text-primary group-hover:opacity-100 focus:opacity-100"
        >
          <Pencil size={12} />
        </button>
      )}
    </div>
  );
}

function SpeakerControl({ segment, defaultSpeaker, editing, value, saving, onToggle, onValue, onSave, onReset }: {
  segment: DisplaySegment;
  defaultSpeaker: string;
  editing: boolean;
  value: string;
  saving: boolean;
  onToggle: () => void;
  onValue: (value: string) => void;
  onSave: (applyToSource: boolean) => Promise<void>;
  onReset: () => Promise<void>;
}) {
  return <div className="relative min-w-0 pt-0.5">
    <button
      onClick={onToggle}
      aria-label={`Rename speaker at ${segment.time}`}
      className={cn(
        "block max-w-full truncate rounded-[var(--radius-s)] px-1 text-left text-xs font-semibold",
        "transition-colors duration-[var(--dur-2)] ease-out hover:bg-surface-3",
        // `text-teal` is the role token (`--color-teal`), which the light theme
        // remaps; the stock `teal-300` scale is not, and read at 1.4:1 on paper.
        segment.source === "mic" ? "text-amber-300" : "text-teal"
      )}
    >
      {segment.speaker}
    </button>
    {editing && <div className="absolute left-0 top-7 z-30 w-60 rounded-[var(--radius-l)] border border-border-hover bg-surface-1 p-3 shadow-[var(--shadow-lg)]">
      <label className="eyebrow block">Speaker name
        <input
          autoFocus
          value={value}
          maxLength={80}
          onChange={(event) => onValue(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Escape") onToggle();
            if (event.key === "Enter") void onSave(false);
          }}
          placeholder={defaultSpeaker}
          aria-label={`Speaker name at ${segment.time}`}
          className="mt-1.5 h-[var(--control-h-s)] w-full rounded-[var(--radius-m)] border border-border bg-surface-2 px-2 text-xs normal-case tracking-normal text-text-primary outline-none focus:border-amber-500/40"
        />
      </label>
      <div className="mt-2.5 flex flex-wrap items-center gap-1.5">
        <Button size="sm" variant="primary" disabled={saving} onClick={() => void onSave(false)}>This line</Button>
        <Button size="sm" variant="secondary" disabled={saving || !value.trim()} onClick={() => void onSave(true)}>All {defaultSpeaker}</Button>
        {segment.speaker_label && <Button size="sm" variant="ghost" className="ml-auto" disabled={saving} onClick={() => void onReset()}>Reset</Button>}
      </div>
      <p className="mt-2.5 text-xs leading-relaxed text-text-muted">Bulk naming is useful for 1:1 calls. Multi-person meeting audio can be labeled one line at a time.</p>
    </div>}
  </div>;
}

function HighlightedText({ text, query }: { text: string; query?: string }) {
  if (!query) return text;
  const lower = text.toLocaleLowerCase();
  const parts: ReactNode[] = [];
  let cursor = 0;
  let match = lower.indexOf(query);
  while (match !== -1) {
    if (match > cursor) parts.push(text.slice(cursor, match));
    parts.push(<mark key={`${match}:${query}`} className="rounded-sm bg-amber-300/20 px-0.5 text-amber-100 light:text-amber-800">{text.slice(match, match + query.length)}</mark>);
    cursor = match + query.length;
    match = lower.indexOf(query, cursor);
  }
  if (cursor < text.length) parts.push(text.slice(cursor));
  return parts;
}

interface StructuredMeetingSummary {
  overview: string;
  topics: Array<{ title: string; details: string; segment_refs: string[] }>;
  decisions: Array<{ text: string; segment_refs: string[] }>;
  action_items: Array<{ task: string; owner: string | null; due: string | null; segment_refs: string[] }>;
  open_questions: string[];
  highlights: string[];
}

function parseMeetingSummary(raw: string | null | undefined): StructuredMeetingSummary | null {
  if (!raw) return null;
  try {
    const value = JSON.parse(raw) as Record<string, unknown>;
    if (typeof value.overview !== "string") return null;
    const strings = (input: unknown) => Array.isArray(input) ? input.filter((item): item is string => typeof item === "string") : [];
    return {
      overview: value.overview,
      topics: Array.isArray(value.topics) ? value.topics.flatMap((item) => {
        const topic = item as Record<string, unknown>;
        return typeof topic.title === "string" && typeof topic.details === "string" ? [{ title: topic.title, details: topic.details, segment_refs: strings(topic.segment_refs) }] : [];
      }) : [],
      decisions: Array.isArray(value.decisions) ? value.decisions.flatMap((item) => {
        const decision = item as Record<string, unknown>;
        return typeof decision.text === "string" ? [{ text: decision.text, segment_refs: strings(decision.segment_refs) }] : [];
      }) : [],
      action_items: Array.isArray(value.action_items) ? value.action_items.flatMap((item) => {
        const action = item as Record<string, unknown>;
        return typeof action.task === "string" ? [{ task: action.task, owner: typeof action.owner === "string" ? action.owner : null, due: typeof action.due === "string" ? action.due : null, segment_refs: strings(action.segment_refs) }] : [];
      }) : [],
      open_questions: strings(value.open_questions),
      highlights: strings(value.highlights),
    };
  } catch {
    return null;
  }
}

/** Owner / due / origin metadata chip — repeated across both action-item views. */
function ActionChip({
  icon,
  tone = "neutral",
  children,
}: {
  icon?: ReactNode;
  tone?: "neutral" | "amber" | "quiet";
  children: ReactNode;
}) {
  return (
    <span
      className={cn(
        "flex items-center gap-1 rounded-[var(--radius-s)] px-1.5 py-0.5 text-xs",
        tone === "amber" && "bg-amber-500/[0.1] text-amber-300",
        tone === "neutral" && "bg-surface-3 text-text-muted",
        tone === "quiet" && "bg-surface-3/70 text-text-muted"
      )}
    >
      {icon}
      {children}
    </span>
  );
}

function StructuredSummary({
  summary,
  actionItems,
  onToggleAction,
  onReference,
}: {
  summary: StructuredMeetingSummary;
  actionItems: MeetingActionItem[];
  onToggleAction: (item: MeetingActionItem, completed: boolean) => void;
  onReference: (reference: string) => void;
}) {
  return (
    <article className="min-h-0 flex-1 select-text space-y-6 overflow-y-auto pr-1 text-sm text-text-secondary">
      <section>
        <h2 className="text-base font-semibold tracking-[-0.01em] text-text-primary">Overview</h2>
        <p className="mt-2 leading-6">{summary.overview}</p>
      </section>

      {actionItems.length > 0 && (
        <SummarySection title="Action items">
          <div className="space-y-2">
            {actionItems.map((item) => (
              <div key={item.id} className="flex gap-2.5 rounded-[var(--radius-l)] border border-border bg-surface-2/40 p-3">
                <Checkbox
                  checked={item.completed}
                  onChange={(checked) => onToggleAction(item, checked)}
                  aria-label={`${item.completed ? "Reopen" : "Complete"} ${item.task}`}
                  className="mt-0.5"
                />
                <div className="min-w-0 flex-1">
                  <p className={cn("leading-5", item.completed ? "text-text-muted line-through" : "text-text-primary")}>
                    {item.task}
                  </p>
                  {(item.owner || item.due) && (
                    <div className="mt-1.5 flex flex-wrap gap-1.5">
                      {item.owner && <ActionChip>{item.owner}</ActionChip>}
                      {item.due && <ActionChip tone="amber">Due {item.due}</ActionChip>}
                    </div>
                  )}
                  <ReferencePills references={item.segment_refs} onReference={onReference} />
                </div>
              </div>
            ))}
          </div>
        </SummarySection>
      )}

      {summary.decisions.length > 0 && (
        <SummarySection title="Decisions">
          <div className="space-y-2">
            {summary.decisions.map((item, index) => (
              <div
                key={`${item.text}:${index}`}
                className="rounded-[var(--radius-l)] border border-success/15 bg-success/[0.04] p-3"
              >
                <p className="leading-5 text-text-primary">{item.text}</p>
                <ReferencePills references={item.segment_refs} onReference={onReference} />
              </div>
            ))}
          </div>
        </SummarySection>
      )}

      {summary.topics.length > 0 && (
        <SummarySection title="Topics">
          <div className="space-y-4">
            {summary.topics.map((item, index) => (
              <div key={`${item.title}:${index}`}>
                <h3 className="text-sm font-semibold text-text-primary">{item.title}</h3>
                <p className="mt-1 leading-6">{item.details}</p>
                <ReferencePills references={item.segment_refs} onReference={onReference} />
              </div>
            ))}
          </div>
        </SummarySection>
      )}

      {summary.open_questions.length > 0 && (
        <SummarySection title="Open questions">
          <ul className="space-y-2">
            {summary.open_questions.map((item, index) => (
              <li
                key={`${item}:${index}`}
                className="flex gap-2 leading-6 before:mt-2.5 before:h-1 before:w-1 before:shrink-0 before:rounded-full before:bg-amber-400"
              >
                {item}
              </li>
            ))}
          </ul>
        </SummarySection>
      )}

      {summary.highlights.length > 0 && (
        <SummarySection title="Highlights">
          <ul className="space-y-2">
            {summary.highlights.map((item, index) => (
              <li
                key={`${item}:${index}`}
                className="flex gap-2 leading-6 before:mt-2.5 before:h-1 before:w-1 before:shrink-0 before:rounded-full before:bg-violet-400"
              >
                {item}
              </li>
            ))}
          </ul>
        </SummarySection>
      )}
    </article>
  );
}

function SummarySection({ title, children }: { title: string; children: ReactNode }) {
  return <section><h2 className="eyebrow mb-2">{title}</h2>{children}</section>;
}

function SummaryHistoryPanel({
  versions,
  currentMarkdown,
  loading,
  restoringId,
  compact,
  onRestore,
  onClose,
}: {
  versions: MeetingSummaryVersion[];
  currentMarkdown: string;
  loading: boolean;
  restoringId: string | null;
  compact?: boolean;
  onRestore: (version: MeetingSummaryVersion) => void;
  onClose: () => void;
}) {
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const currentVersionId = versions.find((version) => version.markdown === currentMarkdown)?.id ?? null;

  return (
    <aside
      className={cn(
        "shrink-0 overflow-y-auto rounded-[var(--radius-l)] border border-border bg-surface-0/60",
        compact ? "max-h-72 w-full" : "w-72"
      )}
    >
      <div className="sticky top-0 z-10 flex items-start gap-2 border-b border-border bg-surface-1/95 p-3 backdrop-blur">
        <History size={14} className="mt-0.5 shrink-0 text-amber-400" />
        <div className="min-w-0 flex-1">
          <h3 className="text-xs font-semibold text-text-primary">Notes history</h3>
          <p className="mt-1 text-xs leading-relaxed text-text-muted">
            Keeps the latest 50 generated, restored, and hand-edited snapshots.
          </p>
        </div>
        <button
          onClick={onClose}
          aria-label="Close notes history"
          className="rounded-[var(--radius-s)] p-1 text-text-muted hover:bg-surface-2 hover:text-text-primary"
        >
          <X size={12} />
        </button>
      </div>

      {loading ? (
        <div className="flex min-h-32 items-center justify-center">
          <Loader2 size={15} className="animate-spin text-text-muted" />
        </div>
      ) : versions.length === 0 ? (
        <p className="p-4 text-center text-xs leading-relaxed text-text-muted">
          The first version is saved when notes are generated.
        </p>
      ) : (
        <div className="space-y-2 p-2">
          {versions.map((version) => {
            const current = version.id === currentVersionId;
            const preview = version.markdown
              .replace(/^#+\s+/gm, "")
              .split(/\n+/)
              .map((line) => line.trim())
              .find(Boolean) ?? "Empty notes";
            const label = version.origin === "manual"
              ? "Edited"
              : version.origin === "restored" ? "Restored" : "Generated";
            const stamp = new Date(version.created_at).toLocaleString();

            return (
              <article
                key={version.id}
                className={cn(
                  "rounded-[var(--radius-m)] border p-2.5",
                  current ? "border-amber-500/25 bg-amber-500/[0.055]" : "border-border bg-surface-1/60"
                )}
              >
                <div className="flex items-center gap-1.5">
                  <Badge tone={version.origin === "manual" ? "violet" : version.origin === "restored" ? "green" : "blue"}>
                    {label}
                  </Badge>
                  {current && <Badge tone="amber">Current</Badge>}
                </div>

                {expandedId === version.id ? (
                  <div className="mt-2 max-h-64 select-text overflow-y-auto whitespace-pre-wrap rounded-[var(--radius-s)] bg-surface-0/60 p-2 text-xs leading-relaxed text-text-secondary">
                    {version.markdown}
                  </div>
                ) : (
                  <p className="mt-2 line-clamp-3 select-text text-xs leading-relaxed text-text-secondary">{preview}</p>
                )}

                <div className="tnum mt-2 space-y-0.5 font-mono text-xs leading-4 text-text-muted">
                  <div>{stamp}</div>
                  {version.model && <div className="truncate" title={version.model}>{version.model}</div>}
                  {version.cost != null && (
                    <div>
                      {formatEstimateCost(version.cost)} ·{" "}
                      {((version.prompt_tokens ?? 0) + (version.completion_tokens ?? 0)).toLocaleString()} tokens
                    </div>
                  )}
                </div>

                <div className="mt-2.5 flex gap-1.5">
                  <Button
                    size="sm"
                    variant="secondary"
                    className="flex-1"
                    onClick={() => setExpandedId((value) => value === version.id ? null : version.id)}
                    aria-label={`${expandedId === version.id ? "Collapse" : "Preview"} notes version from ${stamp}`}
                  >
                    {expandedId === version.id ? "Collapse" : "Preview"}
                  </Button>
                  <Button
                    size="sm"
                    variant="secondary"
                    className="flex-[1.5]"
                    disabled={current || restoringId != null}
                    onClick={() => onRestore(version)}
                    aria-label={
                      current
                        ? `Current notes version from ${stamp}`
                        : `Restore notes version from ${stamp}`
                    }
                    icon={restoringId === version.id ? <Loader2 className="animate-spin" /> : <RotateCcw />}
                  >
                    {current ? "Current" : restoringId === version.id ? "Restoring" : "Restore"}
                  </Button>
                </div>
              </article>
            );
          })}
        </div>
      )}
    </aside>
  );
}

function ReferencePills({ references, onReference }: { references: string[]; onReference: (reference: string) => void }) {
  if (references.length === 0) return null;
  return (
    <div className="mt-2 flex flex-wrap gap-1">
      {references.map((reference) => (
        <button
          key={reference}
          onClick={() => onReference(reference)}
          title="Jump to transcript"
          className="tnum rounded-[var(--radius-s)] bg-surface-3 px-1.5 py-0.5 font-mono text-xs text-text-muted transition-colors duration-[var(--dur-2)] ease-out hover:bg-amber-500/[0.12] hover:text-amber-300"
        >
          {reference.replace(/[[\]]/g, "")}
        </button>
      ))}
    </div>
  );
}

export function localActionDueDate(daysFromToday: number, now = new Date()) {
  const date = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  date.setDate(date.getDate() + daysFromToday);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

function ActionItemsPanel({
  items,
  compact,
  onSave,
  onToggle,
  onDelete,
  onReference,
}: {
  items: MeetingActionItem[];
  compact?: boolean;
  onSave: (current: MeetingActionItem | null, task: string, owner: string, due: string) => Promise<boolean>;
  onToggle: (item: MeetingActionItem, completed: boolean) => void;
  onDelete: (item: MeetingActionItem) => void;
  onReference: (reference: string) => void;
}) {
  const [editing, setEditing] = useState<MeetingActionItem | null>(null);
  const [creating, setCreating] = useState(false);
  const [task, setTask] = useState("");
  const [owner, setOwner] = useState("");
  const [due, setDue] = useState("");
  const [saving, setSaving] = useState(false);

  const reset = () => {
    setEditing(null);
    setCreating(false);
    setTask("");
    setOwner("");
    setDue("");
  };
  const startCreate = () => {
    reset();
    setCreating(true);
  };
  const startEdit = (item: MeetingActionItem) => {
    setEditing(item);
    setCreating(false);
    setTask(item.task);
    setOwner(item.owner ?? "");
    setDue(item.due ?? "");
  };
  const submit = async () => {
    if (!task.trim() || saving) return;
    setSaving(true);
    const saved = await onSave(editing, task, owner, due);
    setSaving(false);
    if (saved) reset();
  };
  const formOpen = creating || editing != null;
  const openItems = items.filter((item) => !item.completed);
  const completedItems = items.filter((item) => item.completed);

  /** Task / Owner / Due share one field recipe. */
  const FORM_FIELD = "mt-1.5 h-9 w-full rounded-[var(--radius-m)] border border-border-hover bg-surface-1 px-2.5 text-xs normal-case tracking-normal text-text-primary outline-none focus:border-amber-500/50";

  return (
    <div className={cn("min-h-full", compact ? "p-3" : "p-5")}>
      <div className="mx-auto max-w-3xl">
        <div className="flex items-start gap-3">
          <div className="min-w-0 flex-1">
            <h2 className="text-sm font-semibold text-text-primary">Meeting follow-ups</h2>
            <p className="mt-1 select-text text-xs leading-relaxed text-text-muted">
              Generated actions and your own additions live here independently from the AI notes. Editing an
              AI action makes your version authoritative when notes are regenerated.
            </p>
          </div>
          <Button size="sm" onClick={startCreate} disabled={formOpen} icon={<Plus />}>Add follow-up</Button>
        </div>

        {formOpen && (
          <div className="mt-4 rounded-[var(--radius-l)] border border-amber-500/20 bg-amber-500/[0.045] p-4">
            <label className="eyebrow block">
              Task
              <input
                autoFocus
                value={task}
                maxLength={500}
                onChange={(event) => setTask(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Escape") reset();
                  if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) void submit();
                }}
                aria-label="Action item task"
                placeholder="Send the revised launch brief"
                className={FORM_FIELD}
              />
            </label>
            <div className={cn("mt-3 grid gap-3", compact ? "grid-cols-1" : "grid-cols-2")}>
              <label className="eyebrow block">
                Owner
                <input
                  value={owner}
                  maxLength={200}
                  onChange={(event) => setOwner(event.target.value)}
                  aria-label="Action item owner"
                  placeholder="Alex"
                  className={FORM_FIELD}
                />
              </label>
              <div>
                <label className="eyebrow block">
                  Due
                  <input
                    value={due}
                    maxLength={200}
                    onChange={(event) => setDue(event.target.value)}
                    aria-label="Action item due date"
                    placeholder="Friday or 2026-08-21"
                    className={FORM_FIELD}
                  />
                </label>
                <div className="mt-2 flex gap-1">
                  {([[0, "Today"], [1, "Tomorrow"], [7, "+1 week"]] as const).map(([days, label]) => (
                    <button
                      key={label}
                      type="button"
                      onClick={() => setDue(localActionDueDate(days))}
                      aria-label={`Set due date to ${label.toLowerCase()}`}
                      className="rounded-[var(--radius-s)] bg-surface-2 px-1.5 py-0.5 text-xs text-text-muted transition-colors duration-[var(--dur-2)] ease-out hover:bg-surface-3 hover:text-amber-300"
                    >
                      {label}
                    </button>
                  ))}
                </div>
              </div>
            </div>
            <div className="mt-4 flex items-center justify-end gap-2">
              <Button size="sm" variant="ghost" onClick={reset}>Cancel</Button>
              <Button
                size="sm"
                disabled={saving || !task.trim()}
                onClick={() => void submit()}
                icon={saving ? <Loader2 className="animate-spin" /> : <Check />}
              >
                {editing ? "Save changes" : "Create follow-up"}
              </Button>
            </div>
          </div>
        )}

        {items.length === 0 && !formOpen ? (
          <div className="flex min-h-72 flex-col items-center justify-center text-center">
            <div className="flex h-11 w-11 items-center justify-center rounded-full bg-surface-2 text-text-muted">
              <ListTodo size={19} />
            </div>
            <h3 className="mt-4 text-sm font-semibold text-text-primary">No follow-ups yet</h3>
            <p className="mt-1 max-w-sm text-xs leading-relaxed text-text-muted">
              Add one manually now, or generated action items will appear here after the meeting summary is ready.
            </p>
            <Button size="sm" variant="secondary" className="mt-4" onClick={startCreate} icon={<Plus />}>
              Add the first one
            </Button>
          </div>
        ) : (
          <div className="mt-4 space-y-5">
            <ActionItemGroup
              title={`${openItems.length} open`}
              items={openItems}
              editingId={editing?.id ?? null}
              onToggle={onToggle}
              onEdit={startEdit}
              onDelete={onDelete}
              onReference={onReference}
            />
            {completedItems.length > 0 && (
              <ActionItemGroup
                title={`${completedItems.length} completed`}
                items={completedItems}
                editingId={editing?.id ?? null}
                onToggle={onToggle}
                onEdit={startEdit}
                onDelete={onDelete}
                onReference={onReference}
              />
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function ActionItemGroup({
  title,
  items,
  editingId,
  onToggle,
  onEdit,
  onDelete,
  onReference,
}: {
  title: string;
  items: MeetingActionItem[];
  editingId: string | null;
  onToggle: (item: MeetingActionItem, completed: boolean) => void;
  onEdit: (item: MeetingActionItem) => void;
  onDelete: (item: MeetingActionItem) => void;
  onReference: (reference: string) => void;
}) {
  if (items.length === 0) return null;
  return (
    <section>
      <h3 className="eyebrow mb-2">{title}</h3>
      <div className="space-y-2">
        {items.map((item) => (
          <article
            key={item.id}
            className={cn(
              "group flex items-start gap-3 rounded-[var(--radius-l)] border p-3",
              "transition-colors duration-[var(--dur-2)] ease-out",
              item.completed ? "border-border bg-surface-2/25" : "border-border bg-surface-2/45 hover:border-border-hover",
              editingId === item.id && "border-amber-500/30"
            )}
          >
            <Checkbox
              checked={item.completed}
              onChange={(checked) => onToggle(item, checked)}
              aria-label={`${item.completed ? "Reopen" : "Complete"} ${item.task}`}
              className="mt-1"
            />
            <div className="min-w-0 flex-1">
              <p className={cn("select-text text-sm leading-5", item.completed ? "text-text-muted line-through" : "text-text-primary")}>
                {item.task}
              </p>
              <div className="mt-2 flex flex-wrap items-center gap-1.5 text-xs text-text-muted">
                {item.owner && <ActionChip icon={<UserRound size={11} />}>{item.owner}</ActionChip>}
                {item.due && <ActionChip tone="amber" icon={<CalendarDays size={11} />}>{item.due}</ActionChip>}
                <ActionChip tone="quiet">{item.origin === "ai" ? "AI suggested" : "Manually managed"}</ActionChip>
              </div>
              <ReferencePills references={item.segment_refs} onReference={onReference} />
            </div>
            <div className="flex shrink-0 opacity-0 transition-opacity duration-[var(--dur-2)] ease-out group-hover:opacity-100 group-focus-within:opacity-100">
              <button
                onClick={() => onEdit(item)}
                aria-label={`Edit ${item.task}`}
                className="rounded-[var(--radius-s)] p-1.5 text-text-muted hover:bg-surface-3 hover:text-text-primary"
              >
                <Pencil size={12} />
              </button>
              <button
                onClick={() => onDelete(item)}
                aria-label={`Delete ${item.task}`}
                className="rounded-[var(--radius-s)] p-1.5 text-text-muted hover:bg-recording-500/10 hover:text-recording-300"
              >
                <Trash2 size={12} />
              </button>
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}

const ASK_SUGGESTIONS = [
  "What decisions were made?",
  "What do I need to follow up on?",
  "What questions remain unresolved?",
];

function QuestionPanel({
  exchanges,
  question,
  busy,
  compact,
  unavailable,
  onQuestion,
  onAsk,
  onDelete,
  onReference,
}: {
  exchanges: MeetingDetail["questions"];
  question: string;
  busy: boolean;
  compact?: boolean;
  unavailable: boolean;
  onQuestion: (value: string) => void;
  onAsk: () => void;
  onDelete: (id: string) => void;
  onReference: (reference: string) => void;
}) {
  return (
    <div className="flex min-h-full flex-col">
      <div className={cn("min-h-0 flex-1 space-y-3", compact ? "p-3" : "p-5")}>
        {exchanges.length === 0 ? (
          <div className="flex min-h-52 flex-col items-center justify-center text-center">
            <div className="flex h-11 w-11 items-center justify-center rounded-full bg-indigo-500/[0.12] text-indigo-300">
              <CircleHelp size={19} />
            </div>
            <h3 className="mt-4 text-sm font-semibold text-text-primary">Ask about this meeting</h3>
            <p className="mt-1.5 max-w-sm text-xs leading-relaxed text-text-muted">
              Answers use the transcript, your notes, and marked moments. Long meetings are searched locally
              first, so only the most useful excerpts are sent. Timestamp sources jump back to the exact passage.
            </p>
            {!unavailable && (
              <div className="mt-4 flex flex-wrap justify-center gap-1.5">
                {ASK_SUGGESTIONS.map((suggestion) => (
                  <button
                    key={suggestion}
                    onClick={() => onQuestion(suggestion)}
                    className="rounded-[var(--radius-m)] border border-border bg-surface-2/50 px-2.5 py-1.5 text-xs text-text-secondary transition-colors duration-[var(--dur-2)] ease-out hover:border-border-hover hover:text-text-primary"
                  >
                    {suggestion}
                  </button>
                ))}
              </div>
            )}
          </div>
        ) : (
          exchanges.map((exchange) => (
            <article key={exchange.id} className="group rounded-[var(--radius-l)] border border-border bg-surface-2/40 p-4">
              <div className="flex items-start gap-2.5">
                <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded-[var(--radius-m)] bg-surface-3 font-mono text-xs font-semibold text-text-secondary">
                  Y
                </div>
                <p className="min-w-0 flex-1 select-text pt-0.5 text-sm font-medium leading-5 text-text-primary">
                  {exchange.question}
                </p>
                <button
                  onClick={() => onDelete(exchange.id)}
                  aria-label={`Delete answer to ${exchange.question}`}
                  className="rounded-[var(--radius-s)] p-1 text-text-muted opacity-0 transition-opacity duration-[var(--dur-2)] ease-out hover:bg-recording-500/10 hover:text-recording-300 group-hover:opacity-100 focus:opacity-100"
                >
                  <Trash2 size={12} />
                </button>
              </div>
              <div className={cn("mt-3 select-text whitespace-pre-wrap text-sm leading-6 text-text-secondary", !compact && "ml-8")}>
                {exchange.answer}
              </div>
              <div className={cn("mt-2 flex flex-wrap items-end gap-2", !compact && "ml-8")}>
                <ReferencePills references={exchange.segment_refs} onReference={onReference} />
                <span className="tnum ml-auto whitespace-nowrap font-mono text-xs text-text-muted">
                  {exchange.total_segments > 0 && (
                    <>
                      {exchange.context_segments === exchange.total_segments
                        ? "full transcript"
                        : `${exchange.context_segments}/${exchange.total_segments} excerpts`}{" "}
                      ·{" "}
                    </>
                  )}
                  {exchange.model} · {formatEstimateCost(exchange.cost)}
                </span>
              </div>
            </article>
          ))
        )}
      </div>

      <div className="sticky bottom-0 border-t border-border bg-surface-1/95 p-3 backdrop-blur">
        <div className="flex items-end gap-2 rounded-[var(--radius-l)] border border-border-hover bg-surface-2 p-2 transition-colors duration-[var(--dur-2)] ease-out focus-within:border-amber-500/40">
          <textarea
            value={question}
            onChange={(event) => onQuestion(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); onAsk(); }
            }}
            maxLength={1000}
            rows={2}
            disabled={unavailable || busy}
            aria-label="Ask a question about this meeting"
            placeholder={unavailable ? "Available after the transcript finishes" : "Ask a follow-up question…"}
            className="min-h-10 min-w-0 flex-1 resize-none bg-transparent px-1 py-1 text-xs leading-5 text-text-primary outline-none placeholder:text-text-muted/60 disabled:cursor-not-allowed"
          />
          <Button
            size="sm"
            variant="primary"
            className="w-8 px-0"
            onClick={onAsk}
            disabled={unavailable || busy || !question.trim()}
            aria-label="Ask meeting"
            icon={busy ? <Loader2 className="animate-spin" /> : <Send />}
          />
        </div>
        <div className="mt-2 flex justify-between gap-3 px-1 text-xs text-text-muted">
          <span>Relevant excerpts are selected locally and sent through your configured privacy routing.</span>
          <span className="tnum shrink-0 font-mono">{question.length}/1000</span>
        </div>
      </div>
    </div>
  );
}

function formatSegmentReference(milliseconds: number) {
  const total = Math.floor(milliseconds / 1000);
  return `[${Math.floor(total / 3600).toString().padStart(2, "0")}:${Math.floor((total % 3600) / 60).toString().padStart(2, "0")}:${(total % 60).toString().padStart(2, "0")}]`;
}

function formatManagedActions(items: MeetingActionItem[]) {
  if (items.length === 0) return "";
  const lines = items.map((item) => {
    const metadata = [item.owner ? `Owner: ${item.owner}` : "", item.due ? `Due: ${item.due}` : ""].filter(Boolean);
    const references = item.segment_refs.length > 0 ? ` ${item.segment_refs.join(" ")}` : "";
    return `- [${item.completed ? "x" : " "}] ${item.task}${metadata.length ? ` — ${metadata.join(" · ")}` : ""}${references}`;
  });
  return `\n\n## Managed follow-ups (authoritative)\n\n${lines.join("\n")}`;
}

function NotesState({ tone, icon, title, description, children }: { tone: string; icon: ReactNode; title: string; description: ReactNode; children?: ReactNode }) {
  return <div className="flex h-full min-h-64 flex-col items-center justify-center px-4 py-8 text-center">
    <div className={cn("flex h-11 w-11 items-center justify-center rounded-full", tone)}>{icon}</div>
    <h3 className="mt-4 text-sm font-semibold text-text-primary">{title}</h3>
    <p className="mt-1.5 max-w-sm select-text text-xs leading-relaxed text-text-muted">{description}</p>
    {children}
  </div>;
}

/**
 * The post-meeting call to action and every state it can be in:
 * live → queued/idle → generating → blocked → failed. Auto-summarize users
 * skip the CTA and land straight in the generating state, because the panel
 * is driven by the meeting's own status rather than by who pressed what.
 */
function AiNotesPanel({
  status,
  recording,
  busy,
  queued,
  transcription,
  modelName,
  estimate,
  estimateLoading,
  estimateError,
  error,
  onGenerate,
  onCancelQueue,
}: {
  status: MeetingDetail["status"];
  recording: boolean;
  busy: boolean;
  queued: boolean;
  transcription: MeetingDetail["transcription"];
  modelName: string | null;
  estimate: MeetingSummaryEstimate | null;
  estimateLoading: boolean;
  estimateError: string | null;
  error: string | null;
  onGenerate: () => void;
  onCancelQueue: () => void;
}) {
  const live = recording || status === "recording" || status === "paused";
  const working = busy || status === "summarizing";
  const transcribing = !live && status === "transcribing";
  const failed = !working && !live && (status === "error" || status === "failed");

  if (live) return <NotesState
    tone="bg-recording-500/[0.12] text-recording-400"
    icon={<Mic size={19} />}
    title="Recording in progress"
    description="End the meeting and OmniVox transcribes it locally. Nothing is sent to a model until you ask for notes."
  />;

  if (working) return <NotesState
    tone="bg-violet-500/[0.12] text-violet-300"
    icon={<Loader2 size={19} className="animate-spin" />}
    title="Writing your AI notes"
    description="Pulling decisions, action items, and open questions out of the transcript and your own notes."
  >
    {modelName && <p className="mt-3 truncate font-mono text-xs text-text-muted">{modelName}</p>}
  </NotesState>;

  if (!working && estimate && !estimate.allowed) return <NotesState
    tone="bg-amber-500/[0.12] text-amber-300"
    icon={<AlertTriangle size={19} />}
    title="Outside your AI spending limits"
    description="Raise the per-meeting or monthly budget in AI provider settings, or pick a cheaper model, to generate these notes."
  >
    <div className="mt-4 w-full max-w-sm"><SummaryEstimateCard estimate={estimate} loading={false} error={null} /></div>
  </NotesState>;

  return <div className="flex h-full min-h-64 flex-col items-center justify-center px-4 py-8 text-center">
    <div className={cn("flex h-11 w-11 items-center justify-center rounded-full", failed ? "bg-recording-500/[0.12] text-recording-400" : "bg-amber-500/[0.12] text-amber-300")}>{failed ? <AlertTriangle size={19} /> : <Sparkles size={19} />}</div>
    <h3 className="mt-4 text-base font-semibold tracking-[-0.01em] text-text-primary">{failed ? "AI notes could not be generated" : "Generate AI notes"}</h3>
    <p className="mt-1.5 max-w-sm select-text text-xs leading-relaxed text-text-muted">
      {failed ? error ?? "The last attempt did not finish. Nothing was lost — the transcript is still here."
        : transcribing ? "The transcript is still finishing. Start now and OmniVox runs the moment the last chunk lands."
        : "Turn the transcript and your own notes into decisions, action items, and open questions. Audio stays on this device."}
    </p>
    <Button className="mt-5 h-10 px-5 text-sm" disabled={queued} onClick={onGenerate} icon={queued ? <Loader2 className="animate-spin" /> : <Sparkles />}>
      {queued ? "Queued" : failed ? "Try again" : "Generate AI notes"}
    </Button>
    {queued && <p className="mt-2.5 flex items-center gap-1.5 text-xs text-text-muted">Will generate when transcription finishes<MetaDot /><button onClick={onCancelQueue} className="rounded font-medium text-text-secondary hover:text-text-primary">Cancel</button></p>}
    <div className="mt-4 min-h-4 text-xs text-text-muted">
      {estimateLoading ? <span className="flex items-center gap-1.5"><Loader2 size={12} className="animate-spin" /> Verifying current model price</span>
        : estimateError ? <span className="max-w-sm text-amber-200">Price preview unavailable: {estimateError}</span>
        : estimate ? <span className="tnum font-mono">Up to {formatEstimateCost(estimate.estimated_cost)} · {estimate.model_name}</span>
        : modelName ? <span className="truncate font-mono">{modelName}</span>
        : null}
    </div>
    {transcribing && <div className="mt-5 w-full max-w-sm text-left"><TranscriptionProgress progress={transcription} /></div>}
  </div>;
}

function SummaryEstimateBadge({ estimate, loading }: { estimate: MeetingSummaryEstimate | null; loading: boolean }) {
  if (loading) {
    return (
      <span className="flex shrink-0 items-center gap-1 whitespace-nowrap text-xs text-text-muted">
        <Loader2 size={11} className="animate-spin" /> Checking price
      </span>
    );
  }
  if (!estimate) return null;
  return (
    <span
      title={`${estimate.required_context_tokens.toLocaleString()} of ${estimate.context_length.toLocaleString()} context tokens · ${formatEstimateCost(estimate.meeting_spend)} already used by this meeting`}
      className={cn(
        "tnum shrink-0 whitespace-nowrap font-mono text-xs",
        estimate.allowed ? "text-text-muted" : "text-amber-300"
      )}
    >
      Up to {formatEstimateCost(estimate.estimated_cost)}
    </span>
  );
}

function SummaryEstimateCard({
  estimate,
  loading,
  error,
}: {
  estimate: MeetingSummaryEstimate | null;
  loading: boolean;
  error: string | null;
}) {
  if (loading) {
    return (
      <div className="flex items-center gap-2 text-xs text-text-muted">
        <Loader2 size={12} className="animate-spin" /> Verifying current model price
      </div>
    );
  }
  if (error) {
    return (
      <div className="max-w-sm rounded-[var(--radius-l)] border border-amber-500/20 bg-amber-500/[0.06] px-3 py-2 text-xs leading-relaxed text-amber-200">
        Price preview unavailable: {error}
      </div>
    );
  }
  if (!estimate) return null;

  return (
    <div
      className={cn(
        "w-full rounded-[var(--radius-l)] border px-3 py-2.5 text-left",
        estimate.allowed ? "border-success/15 bg-success/[0.05]" : "border-amber-500/20 bg-amber-500/[0.07]"
      )}
    >
      <div className="flex items-center gap-2">
        <CircleDollarSign size={13} className={cn("shrink-0", estimate.allowed ? "text-success" : "text-amber-300")} />
        <span className="min-w-0 flex-1 truncate text-xs font-medium text-text-primary">{estimate.model_name}</span>
        <span className="tnum shrink-0 font-mono text-xs font-semibold text-text-primary">
          Up to {formatEstimateCost(estimate.estimated_cost)}
        </span>
      </div>
      <div className="tnum mt-2 flex flex-wrap justify-between gap-x-3 text-xs text-text-muted">
        <span>
          {estimate.required_context_tokens.toLocaleString()} / {estimate.context_length.toLocaleString()} context tokens
        </span>
        <span>
          {estimate.per_meeting_budget > 0
            ? `${formatEstimateCost(estimate.meeting_spend)} / ${formatEstimateCost(estimate.per_meeting_budget)} this meeting`
            : "No meeting cap"}
        </span>
      </div>
      <div className="tnum mt-1 text-right text-xs text-text-muted/80">
        {estimate.monthly_budget > 0
          ? `${formatEstimateCost(estimate.month_spend)} / ${formatEstimateCost(estimate.monthly_budget)} this month`
          : "No monthly cap"}
      </div>
      {estimate.blocking_reason && (
        <p role="alert" className="mt-2 text-xs leading-relaxed text-amber-200">{estimate.blocking_reason}</p>
      )}
      <p className="mt-2 text-xs leading-relaxed text-text-muted/80">
        Conservative ceiling based on the full output allowance; the final charge is usually lower.
      </p>
    </div>
  );
}

function formatEstimateCost(value: number) {
  return `$${value.toFixed(value < 0.01 ? 4 : 2)}`;
}

function TranscriptionProgress({ progress }: { progress: MeetingDetail["transcription"] }) {
  const finished = progress.completed + progress.failed;
  const percent = progress.total > 0 ? Math.min(100, (finished / progress.total) * 100) : 0;
  const label = progress.total > 0
    ? `Transcribing audio · ${finished} of ${progress.total} chunks finished`
    : "Preparing captured audio";

  return (
    <div className="mt-3 rounded-[var(--radius-l)] border border-violet-500/15 bg-violet-500/[0.05] px-3 py-2.5">
      <div className="flex items-center gap-2 text-xs text-violet-200">
        <Loader2 size={12} className="animate-spin" />
        <span className="tnum min-w-0 flex-1">{label}</span>
        {progress.failed > 0 && <span className="tnum shrink-0 text-amber-300">{progress.failed} failed</span>}
      </div>
      <Progress className="mt-2" accent="violet" value={percent} aria-label={label} />
    </div>
  );
}

function CaptureSourceBadge({ label, detected, paused }: { label: string; detected: boolean; paused: boolean }) {
  return (
    <span
      title={
        paused
          ? `${label} capture is paused`
          : detected ? `${label} audio detected` : `Waiting for ${label.toLocaleLowerCase()} audio`
      }
      className="flex items-center gap-1.5 rounded-[var(--radius-s)] bg-surface-0/60 px-1.5 py-0.5 text-xs text-text-muted"
    >
      <span
        className={cn(
          "h-1.5 w-1.5 rounded-full",
          paused ? "bg-amber-400/70" : detected ? "bg-success" : "bg-text-muted/35"
        )}
      />
      {label}
    </span>
  );
}
