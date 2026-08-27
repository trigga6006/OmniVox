import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  Bookmark,
  CalendarClock,
  ChevronDown,
  ExternalLink,
  History,
  Headphones,
  Loader2,
  Mic2,
  ListChecks,
  PanelRightOpen,
  Search,
  Settings2,
  ShieldCheck,
  Sparkles,
  Star,
  Trash2,
  Undo2,
  Waves,
} from "lucide-react";
import {
  Badge,
  Button,
  Checkbox,
  ConfirmDialog,
  EmptyState,
  Input,
  Modal,
  Progress,
  Select,
} from "@/components/ui";
import {
  addMeetingActionItem,
  listDeletedMeetings,
  listMeetingTemplates,
  listMeetingActionItems,
  listMeetingRecoveryItems,
  listMeetings,
  getAudioDevices,
  getSelectedAudioDevice,
  meetingStart,
  onMeetingUpdated,
  openMeetingDrawer,
  permanentlyDeleteMeeting,
  deleteMeetingTemplate,
  restoreMeeting,
  retryMeetingTranscription,
  searchDeletedMeetings,
  searchMeetings,
  setMeetingFavorite,
  setMeetingActionCompleted,
  setMeetingActionItemCompleted,
  setAudioDevice,
  updateMeetingAiOptions,
  updateMeetingMetadata,
  testMeetingAudio,
  type MeetingAudioReadiness,
  type AudioDevice,
  type Meeting,
  type MeetingActionItem,
  type MeetingRecoveryItem,
  type MeetingTemplate,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { MeetingWorkspace } from "./MeetingWorkspace";
import { extractMeetingFollowUps, MeetingFollowUps, type MeetingFollowUp } from "./MeetingFollowUps";
import { MeetingRecoveryCenter, recoveryPresentation } from "./MeetingRecoveryCenter";
import { findPreviousMeeting } from "./meetingContinuity";
import { OpenRouterPanel } from "./OpenRouterPanel";
import { useMeetingRuntime } from "./useMeetingRuntime";

/** The title a one-tap start uses when the user never typed one. */
export function defaultMeetingTitle(now = new Date()) {
  return `Meeting · ${now.toLocaleDateString()}`;
}

function statusDot(status: string) {
  if (status === "recording" || status === "error" || status === "failed") return "bg-recording-400";
  if (status === "transcribing" || status === "summarizing") return "bg-violet-400";
  if (status === "awaiting_summary" || status === "paused" || status === "interrupted") return "bg-amber-400";
  return "bg-text-muted/45";
}

export function MeetingsPage() {
  const [meetings, setMeetings] = useState<Meeting[]>([]);
  const [actionItems, setActionItems] = useState<MeetingActionItem[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [newMeetingOpen, setNewMeetingOpen] = useState(false);
  const [providerOpen, setProviderOpen] = useState(false);
  const [title, setTitle] = useState("");
  const [agenda, setAgenda] = useState("");
  const [participants, setParticipants] = useState("");
  const [meetingHistory, setMeetingHistory] = useState<Meeting[]>([]);
  const [meetingTemplates, setMeetingTemplates] = useState<MeetingTemplate[]>([]);
  const [templateId, setTemplateId] = useState("");
  const [audioDevices, setAudioDevices] = useState<AudioDevice[]>([]);
  const [selectedAudioDevice, setSelectedAudioDeviceState] = useState("");
  const [audioDevicesLoading, setAudioDevicesLoading] = useState(false);
  const [previousMeetingId, setPreviousMeetingId] = useState("");
  const [previousTouched, setPreviousTouched] = useState(false);
  const [carryAgenda, setCarryAgenda] = useState(true);
  const [carryParticipants, setCarryParticipants] = useState(true);
  const [carryActions, setCarryActions] = useState(true);
  const [carryAiSetup, setCarryAiSetup] = useState(true);
  const [starting, setStarting] = useState(false);
  const [testingAudio, setTestingAudio] = useState(false);
  const [audioReadiness, setAudioReadiness] = useState<MeetingAudioReadiness | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [viewingTrash, setViewingTrash] = useState(false);
  const [followUpsOpen, setFollowUpsOpen] = useState(false);
  const [followUpBusy, setFollowUpBusy] = useState<string | null>(null);
  const [recoveryItems, setRecoveryItems] = useState<MeetingRecoveryItem[]>([]);
  const [recoveryOpen, setRecoveryOpen] = useState(false);
  const [recoveryBusy, setRecoveryBusy] = useState<string | null>(null);
  const [transcriptTarget, setTranscriptTarget] = useState<{ meetingId: string; reference: string } | null>(null);
  const [confirmDeleteTemplate, setConfirmDeleteTemplate] = useState(false);
  const [confirmPurge, setConfirmPurge] = useState(false);
  const deferredQuery = useDeferredValue(query);
  const searchRequest = useRef(0);
  const { runtime } = useMeetingRuntime();

  const refresh = useCallback(() => {
    const request = ++searchRequest.current;
    const load = viewingTrash
      ? (deferredQuery.trim() ? searchDeletedMeetings(deferredQuery) : listDeletedMeetings())
      : (deferredQuery.trim() ? searchMeetings(deferredQuery) : listMeetings());
    Promise.all([
      load,
      viewingTrash ? Promise.resolve([] as MeetingActionItem[]) : listMeetingActionItems(),
      viewingTrash ? Promise.resolve([] as MeetingRecoveryItem[]) : listMeetingRecoveryItems(),
    ]).then(([rows, actions, recovery]) => {
      if (request !== searchRequest.current) return;
      setMeetings(rows);
      setActionItems(actions);
      setRecoveryItems(recovery);
      setSelectedId((current) => current && rows.some((row) => row.id === current) ? current : (!viewingTrash ? runtime.meeting_id : null) ?? rows[0]?.id ?? null);
    }).catch((reason) => setError(String(reason)));
  }, [deferredQuery, runtime.meeting_id, viewingTrash]);

  useEffect(() => {
    refresh();
    const unlisten = onMeetingUpdated(refresh);
    return () => { void unlisten.then((fn) => fn()); };
  }, [refresh]);

  useEffect(() => {
    if (runtime.meeting_id) { setViewingTrash(false); setTranscriptTarget(null); setSelectedId(runtime.meeting_id); }
  }, [runtime.meeting_id]);

  useEffect(() => {
    if (transcriptTarget && transcriptTarget.meetingId !== selectedId) setTranscriptTarget(null);
  }, [selectedId, transcriptTarget]);

  useEffect(() => {
    if (!newMeetingOpen) return;
    setAudioDevicesLoading(true);
    Promise.allSettled([listMeetings(), getAudioDevices(), getSelectedAudioDevice(), listMeetingTemplates(), listMeetingActionItems()])
      .then(([meetingResult, devicesResult, selectedResult, templatesResult, actionsResult]) => {
        if (meetingResult.status === "fulfilled") setMeetingHistory(meetingResult.value);
        else setError(String(meetingResult.reason));
        if (devicesResult.status === "fulfilled") {
          setAudioDevices(devicesResult.value);
          const configured = selectedResult.status === "fulfilled" ? selectedResult.value : null;
          setSelectedAudioDeviceState(configured ?? devicesResult.value.find((device) => device.is_default)?.id ?? devicesResult.value[0]?.id ?? "");
        } else {
          setError(String(devicesResult.reason));
        }
        if (templatesResult.status === "fulfilled") setMeetingTemplates(templatesResult.value);
        else setError(String(templatesResult.reason));
        if (actionsResult.status === "fulfilled") setActionItems(actionsResult.value);
        else setError(String(actionsResult.reason));
      })
      .finally(() => setAudioDevicesLoading(false));
  }, [newMeetingOpen]);

  useEffect(() => {
    if (!newMeetingOpen || previousTouched) return;
    setPreviousMeetingId(findPreviousMeeting(meetingHistory, title)?.id ?? "");
  }, [meetingHistory, newMeetingOpen, previousTouched, title]);

  const previousMeeting = meetingHistory.find((meeting) => meeting.id === previousMeetingId) ?? null;
  const selectedTemplate = meetingTemplates.find((template) => template.id === templateId) ?? null;
  const previousActions = useMemo(
    () => previousMeeting ? extractMeetingFollowUps([previousMeeting], actionItems).filter((item) => !item.completed) : [],
    [actionItems, previousMeeting],
  );

  const openNewMeeting = useCallback(() => {
    setTitle("");
    setAgenda("");
    setParticipants("");
    setPreviousMeetingId("");
    setTemplateId("");
    setPreviousTouched(false);
    setCarryAgenda(true);
    setCarryParticipants(true);
    setCarryActions(true);
    setCarryAiSetup(true);
    setAudioReadiness(null);
    setNewMeetingOpen(true);
  }, []);

  const openMeeting = useCallback((meetingId: string, reference?: string) => {
    setTranscriptTarget(reference ? { meetingId, reference } : null);
    setSelectedId(meetingId);
    setFollowUpsOpen(false);
    setRecoveryOpen(false);
  }, []);

  const applyTemplate = useCallback((id: string) => {
    setTemplateId(id);
    const template = meetingTemplates.find((candidate) => candidate.id === id);
    if (!template) return;
    setTitle(template.title);
    setAgenda(template.agenda);
    setParticipants(template.participants.join(", "));
    setPreviousMeetingId("");
    setPreviousTouched(true);
  }, [meetingTemplates]);

  const removeTemplate = useCallback(async () => {
    setConfirmDeleteTemplate(false);
    if (!selectedTemplate) return;
    try {
      await deleteMeetingTemplate(selectedTemplate.id);
      setMeetingTemplates((current) => current.filter((template) => template.id !== selectedTemplate.id));
      setTemplateId("");
    } catch (reason) {
      setError(String(reason));
    }
  }, [selectedTemplate]);

  const runAudioTest = useCallback(async () => {
    setTestingAudio(true);
    setError(null);
    try {
      setAudioReadiness(await testMeetingAudio());
    } catch (reason) {
      setError(String(reason));
    } finally {
      setTestingAudio(false);
    }
  }, []);

  const changeAudioDevice = useCallback(async (deviceId: string) => {
    const previous = selectedAudioDevice;
    setSelectedAudioDeviceState(deviceId);
    setAudioReadiness(null);
    try {
      await setAudioDevice(deviceId);
    } catch (reason) {
      setSelectedAudioDeviceState(previous);
      setError(String(reason));
    }
  }, [selectedAudioDevice]);

  /**
   * One tap: no dialog, no fields. The meeting is named from today's date and
   * selected immediately — everything else (title, agenda, continuity) stays
   * editable inline in the workspace while it records.
   */
  const quickStart = useCallback(async () => {
    if (starting || runtime.meeting_id) return;
    setStarting(true);
    setError(null);
    try {
      const meeting = await meetingStart(defaultMeetingTitle());
      setViewingTrash(false);
      setFollowUpsOpen(false);
      setRecoveryOpen(false);
      setTranscriptTarget(null);
      setSelectedId(meeting.id);
      refresh();
    } catch (reason) { setError(String(reason)); }
    finally { setStarting(false); }
  }, [refresh, runtime.meeting_id, starting]);

  const start = useCallback(async () => {
    setStarting(true);
    setError(null);
    try {
      const meetingTitle = title.trim() || defaultMeetingTitle();
      const effectiveAgenda = agenda.trim() || (carryAgenda ? previousMeeting?.agenda ?? "" : "");
      const typedParticipants = participants.split(",").map((value) => value.trim()).filter(Boolean);
      const effectiveParticipants = typedParticipants.length > 0
        ? typedParticipants
        : carryParticipants ? previousMeeting?.participants ?? [] : [];
      const meeting = await meetingStart(meetingTitle, undefined, previousMeeting?.id);
      const contextWrites: Promise<unknown>[] = [];
      if (effectiveAgenda || effectiveParticipants.length > 0) {
        contextWrites.push(updateMeetingMetadata(meeting.id, effectiveAgenda, effectiveParticipants));
      }
      if (carryActions && previousMeeting && previousActions.length > 0) {
        contextWrites.push(Promise.all(previousActions.slice(0, 50).map((action) =>
          addMeetingActionItem(meeting.id, action.task, action.owner ?? undefined, action.due ?? undefined),
        )));
      }
      if (selectedTemplate && (selectedTemplate.ai_model_override || selectedTemplate.summary_preset_override || selectedTemplate.summary_instructions.trim())) {
        contextWrites.push(updateMeetingAiOptions(
          meeting.id,
          selectedTemplate.ai_model_override,
          selectedTemplate.summary_preset_override,
          selectedTemplate.summary_instructions,
        ));
      } else if (carryAiSetup && previousMeeting && (previousMeeting.ai_model_override || previousMeeting.summary_preset_override || previousMeeting.summary_instructions.trim())) {
        contextWrites.push(updateMeetingAiOptions(
          meeting.id,
          previousMeeting.ai_model_override,
          previousMeeting.summary_preset_override,
          previousMeeting.summary_instructions,
        ));
      }
      const writeResults = await Promise.allSettled(contextWrites);
      const failedWrites = writeResults.filter((result) => result.status === "rejected");
      if (failedWrites.length > 0) {
        setError(`Meeting started, but ${failedWrites.length} continuity update${failedWrites.length === 1 ? "" : "s"} could not be saved.`);
      }
      setNewMeetingOpen(false);
      setTitle("");
      setAgenda("");
      setParticipants("");
      setPreviousMeetingId("");
      setTemplateId("");
      setPreviousTouched(false);
      setTranscriptTarget(null);
      setSelectedId(meeting.id);
      refresh();
    } catch (reason) { setError(String(reason)); }
    finally { setStarting(false); }
  }, [agenda, carryActions, carryAgenda, carryAiSetup, carryParticipants, participants, previousActions, previousMeeting, refresh, selectedTemplate, title]);

  const selectedMeeting = meetings.find((meeting) => meeting.id === selectedId) ?? null;
  const selectedMicrophoneName = audioDevices.find((device) => device.id === selectedAudioDevice)?.name ?? "Selected microphone";

  const templateOptions = useMemo(() => [
    { value: "", label: "Start without a template" },
    ...meetingTemplates.map((template) => ({ value: template.id, label: template.name })),
  ], [meetingTemplates]);

  const previousMeetingOptions = useMemo(() => [
    { value: "", label: "Start without previous context" },
    ...meetingHistory.slice(0, 20).map((meeting) => ({
      value: meeting.id,
      label: meeting.title,
      hint: new Date(meeting.started_at).toLocaleDateString(),
    })),
  ], [meetingHistory]);

  const microphoneOptions = useMemo(() => audioDevices.length === 0
    ? [{ value: "", label: audioDevicesLoading ? "Finding microphones…" : "No microphone available" }]
    : audioDevices.map((device) => ({
      value: device.id,
      label: `${device.name}${device.is_default ? " · Windows default" : ""}`,
      hint: `${device.sample_rate.toLocaleString()} Hz`,
    })), [audioDevices, audioDevicesLoading]);

  const purgeSelectedMeeting = useCallback(() => {
    setConfirmPurge(false);
    if (!selectedMeeting) return;
    void permanentlyDeleteMeeting(selectedMeeting.id)
      .then(() => { setTranscriptTarget(null); setSelectedId(null); refresh(); })
      .catch((reason) => setError(String(reason)));
  }, [refresh, selectedMeeting]);

  const restoreSelectedMeeting = useCallback(() => {
    if (!selectedMeeting) return;
    void restoreMeeting(selectedMeeting.id)
      .then(() => { setTranscriptTarget(null); setSelectedId(null); refresh(); })
      .catch((reason) => setError(String(reason)));
  }, [refresh, selectedMeeting]);
  const pendingFollowUps = useMemo(
    () => extractMeetingFollowUps(meetings, actionItems).filter((item) => !item.completed).length,
    [actionItems, meetings],
  );
  const retryableRecoveryItems = useMemo(
    () => recoveryItems.filter((item) => recoveryPresentation(item).kind === "retry"),
    [recoveryItems],
  );

  const retryRecovery = useCallback(async (item: MeetingRecoveryItem) => {
    if (recoveryBusy) return;
    setRecoveryBusy(item.meeting_id);
    setError(null);
    try {
      await retryMeetingTranscription(item.meeting_id);
      refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setRecoveryBusy(null);
    }
  }, [recoveryBusy, refresh]);

  const retryAllRecovery = useCallback(async () => {
    if (recoveryBusy || retryableRecoveryItems.length === 0) return;
    setRecoveryBusy("all");
    setError(null);
    const failures: string[] = [];
    for (const item of retryableRecoveryItems) {
      try {
        await retryMeetingTranscription(item.meeting_id);
      } catch (reason) {
        failures.push(`${item.title}: ${String(reason)}`);
      }
    }
    if (failures.length > 0) setError(`Could not restart ${failures.length} meeting${failures.length === 1 ? "" : "s"}. ${failures[0]}`);
    setRecoveryBusy(null);
    refresh();
  }, [recoveryBusy, refresh, retryableRecoveryItems]);

  const toggleFollowUp = useCallback(async (item: MeetingFollowUp, completed: boolean) => {
    const key = `${item.meetingId}:${item.task}`;
    if (followUpBusy) return;
    setFollowUpBusy(key);
    setError(null);
    try {
      if (item.id) {
        const stored = await setMeetingActionItemCompleted(item.id, item.meetingId, completed);
        setActionItems((current) => current.map((candidate) => candidate.id === stored.id ? stored : candidate));
        setMeetings((current) => current.map((meeting) => meeting.id === item.meetingId
          ? { ...meeting, completed_actions: completed ? [...meeting.completed_actions.filter((task) => task !== item.task), item.task] : meeting.completed_actions.filter((task) => task !== item.task) }
          : meeting));
      } else {
        const completedActions = await setMeetingActionCompleted(item.meetingId, item.task, completed);
        setMeetings((current) => current.map((meeting) => meeting.id === item.meetingId
          ? { ...meeting, completed_actions: completedActions }
          : meeting));
      }
    } catch (reason) {
      setError(String(reason));
      refresh();
    } finally {
      setFollowUpBusy(null);
    }
  }, [followUpBusy, refresh]);

  const startDisabled = starting || runtime.meeting_id !== null;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-4">
        <h1 className="shrink-0 text-sm font-semibold tracking-[-0.01em] text-text-primary">Meetings</h1>
        {viewingTrash && <Badge tone="neutral">Trash</Badge>}
        <div className="ml-auto flex items-center gap-1.5">
          {!viewingTrash && recoveryItems.length > 0 && (
            <Button
              size="sm"
              variant={recoveryOpen ? "primary" : "ghost"}
              icon={<ShieldCheck />}
              onClick={() => {
                setTranscriptTarget(null);
                setFollowUpsOpen(false);
                setRecoveryOpen((value) => !value);
              }}
            >
              Recovery · {recoveryItems.length}
            </Button>
          )}
          {!viewingTrash && (
            <Button
              size="sm"
              variant={followUpsOpen ? "primary" : "ghost"}
              icon={<ListChecks />}
              onClick={() => {
                setTranscriptTarget(null);
                setRecoveryOpen(false);
                setFollowUpsOpen((value) => !value);
              }}
            >
              Follow-ups{pendingFollowUps > 0 ? ` · ${pendingFollowUps}` : ""}
            </Button>
          )}
          <Button size="sm" variant="ghost" icon={<Settings2 />} onClick={() => setProviderOpen(true)}>
            AI provider
          </Button>
          <div className="flex items-stretch">
            <Button
              size="sm"
              variant="primary"
              icon={<Mic2 />}
              className="rounded-r-none"
              onClick={() => void quickStart()}
              disabled={startDisabled}
            >
              {starting ? "Starting…" : "Start meeting"}
            </Button>
            <Button
              size="sm"
              variant="primary"
              className="rounded-l-none border-l border-amber-700/40 px-1.5"
              aria-label="Start with options"
              title="Start with options — title, template, continuity, microphone test"
              onClick={openNewMeeting}
              disabled={startDisabled}
              icon={<ChevronDown />}
            />
          </div>
        </div>
      </header>

      {error && (
        <div
          role="alert"
          className="flex shrink-0 items-center gap-2 border-b border-amber-500/20 bg-amber-500/[0.07] px-4 py-2 text-xs text-amber-200"
        >
          <span className="min-w-0 flex-1">{error}</span>
          <button
            onClick={() => setError(null)}
            aria-label="Dismiss meetings error"
            className="shrink-0 rounded px-1 opacity-70 hover:opacity-100"
          >
            ×
          </button>
        </div>
      )}

      <div className="flex min-h-0 flex-1">
        <aside className="flex w-[280px] shrink-0 flex-col border-r border-border bg-surface-0/40">
          <div className="flex h-11 shrink-0 items-center gap-2 border-b border-border px-3 transition-colors focus-within:bg-surface-1/60">
            <Search size={13} className="shrink-0 text-text-muted" />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={viewingTrash ? "Search trash" : "Search notes or transcript"}
              aria-label="Search meetings"
              className="min-w-0 flex-1 bg-transparent text-xs text-text-primary outline-none placeholder:text-text-muted/65"
            />
            <span className="tnum shrink-0 font-mono text-xs text-text-muted">{meetings.length}</span>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto">
            {meetings.length === 0 ? (
              <p className="px-4 py-8 text-center text-xs leading-relaxed text-text-muted">
                {query.trim()
                  ? `No meetings match “${query.trim()}”.`
                  : viewingTrash
                    ? "Trash is empty."
                    : "Recorded meetings will appear here, safely stored on this device."}
              </p>
            ) : (
              meetings.map((meeting) => {
                const live = runtime.meeting_id === meeting.id;
                return (
                  <MeetingListRow
                    key={meeting.id}
                    meeting={meeting}
                    live={live}
                    selected={!followUpsOpen && !recoveryOpen && selectedId === meeting.id}
                    status={viewingTrash ? "deleted" : live ? runtime.status : meeting.status}
                    favoritable={!viewingTrash && !live}
                    onOpen={() => openMeeting(meeting.id)}
                    onToggleFavorite={() => {
                      void setMeetingFavorite(meeting.id, !meeting.is_favorite)
                        .then(refresh)
                        .catch((reason) => setError(String(reason)));
                    }}
                  />
                );
              })
            )}
          </div>
          <div className="shrink-0 border-t border-border p-1.5">
            {!viewingTrash && (
              <Button
                variant="ghost"
                size="sm"
                className="w-full justify-start"
                onClick={() => selectedId && openMeetingDrawer(selectedId)}
                disabled={!selectedId}
                icon={<PanelRightOpen />}
              >
                Open floating drawer
              </Button>
            )}
            <Button
              variant="ghost"
              size="sm"
              className={cn("w-full justify-start", viewingTrash && "text-amber-300")}
              onClick={() => {
                setViewingTrash((value) => !value);
                setFollowUpsOpen(false);
                setRecoveryOpen(false);
                setTranscriptTarget(null);
                setSelectedId(null);
                setQuery("");
              }}
              icon={viewingTrash ? <Undo2 /> : <Trash2 />}
            >
              {viewingTrash ? "Back to meetings" : "Trash"}
            </Button>
          </div>
        </aside>

        <section className="min-w-0 flex-1">
          {recoveryOpen && !viewingTrash ? (
            <MeetingRecoveryCenter
              items={recoveryItems}
              busyId={recoveryBusy}
              onRetry={(item) => void retryRecovery(item)}
              onRetryAll={() => void retryAllRecovery()}
              onOpenMeeting={openMeeting}
            />
          ) : followUpsOpen && !viewingTrash ? (
            <MeetingFollowUps
              meetings={meetings}
              actionItems={actionItems}
              busyKey={followUpBusy}
              onToggle={(item, completed) => void toggleFollowUp(item, completed)}
              onOpenMeeting={openMeeting}
            />
          ) : selectedId && selectedMeeting ? (
            viewingTrash ? (
              <TrashedMeeting
                meeting={selectedMeeting}
                onRestore={restoreSelectedMeeting}
                onDeleteForever={() => setConfirmPurge(true)}
              />
            ) : (
              <MeetingWorkspace
                meetingId={selectedId}
                initialTranscriptReference={
                  transcriptTarget?.meetingId === selectedId ? transcriptTarget.reference : undefined
                }
                onInitialTranscriptReferenceConsumed={() => setTranscriptTarget(null)}
                onDeleted={() => {
                  setTranscriptTarget(null);
                  setSelectedId(null);
                  refresh();
                }}
                onOpenMeeting={openMeeting}
              />
            )
          ) : viewingTrash ? (
            <TrashEmpty />
          ) : (
            <MeetingsEmpty
              starting={starting}
              disabled={startDisabled}
              onStart={() => void quickStart()}
              onOptions={openNewMeeting}
              onProvider={() => setProviderOpen(true)}
            />
          )}
        </section>
      </div>

      <Modal
        open={newMeetingOpen}
        onClose={() => !starting && setNewMeetingOpen(false)}
        title="Start with options"
        description="OmniVox captures your microphone and the default system output. Confirm everyone has consented before recording."
        footer={
          <>
            <Button variant="secondary" onClick={() => setNewMeetingOpen(false)}>Cancel</Button>
            <Button variant="primary" onClick={start} disabled={starting} icon={<Mic2 />}>
              {starting ? "Starting…" : "Start recording"}
            </Button>
          </>
        }
      >
        <div className="space-y-4">
          {meetingTemplates.length > 0 && (
            <DialogField
              label="Reusable setup"
              hint="Templates restore title, agenda, participants, and meeting-specific AI setup."
            >
              <div className="mt-2 flex gap-2">
                <div className="relative min-w-0 flex-1">
                  <Select
                    aria-label="Meeting template"
                    options={templateOptions}
                    value={templateId}
                    onChange={applyTemplate}
                    className="pl-9"
                  />
                  <Bookmark
                    size={13}
                    className="pointer-events-none absolute left-3 top-1/2 z-10 -translate-y-1/2 text-text-muted"
                  />
                </div>
                <Button
                  variant="secondary"
                  size="sm"
                  className="h-[var(--control-h-m)] w-[var(--control-h-m)] shrink-0 px-0"
                  disabled={!selectedTemplate}
                  onClick={() => setConfirmDeleteTemplate(true)}
                  aria-label="Delete selected meeting template"
                  icon={<Trash2 />}
                />
              </div>
            </DialogField>
          )}

          <DialogField label="Meeting title">
            <Input
              autoFocus
              value={title}
              onChange={(event) => setTitle(event.target.value)}
              onKeyDown={(event) => { if (event.key === "Enter") start(); }}
              placeholder="Weekly product sync"
              className="mt-2"
            />
          </DialogField>

          {meetingHistory.length > 0 && (
            <div className="rounded-[var(--radius-l)] border border-border bg-surface-2/40 p-3">
              <div className="flex items-center gap-2">
                <span className="eyebrow flex shrink-0 items-center gap-2"><History size={12} /> Continue from</span>
                <div className="ml-auto min-w-0 max-w-[70%] flex-1">
                  <Select
                    aria-label="Previous meeting"
                    options={previousMeetingOptions}
                    value={previousMeetingId}
                    onChange={(value) => { setPreviousTouched(true); setPreviousMeetingId(value); }}
                    className="h-[var(--control-h-s)] text-xs"
                  />
                </div>
              </div>
              {previousMeeting && (
                <div className="mt-3 space-y-2 border-t border-border pt-3">
                  <CarryOption
                    checked={carryParticipants}
                    onChange={setCarryParticipants}
                    disabled={previousMeeting.participants.length === 0}
                    label={`Reuse ${previousMeeting.participants.length} participant${previousMeeting.participants.length === 1 ? "" : "s"}`}
                    detail={previousMeeting.participants.slice(0, 4).join(", ")}
                  />
                  <CarryOption
                    checked={carryAgenda}
                    onChange={setCarryAgenda}
                    disabled={!previousMeeting.agenda.trim()}
                    label="Reuse prior agenda"
                    detail={previousMeeting.agenda}
                  />
                  <CarryOption
                    checked={carryActions}
                    onChange={setCarryActions}
                    disabled={previousActions.length === 0}
                    label={`Carry ${previousActions.length} unfinished follow-up${previousActions.length === 1 ? "" : "s"} as editable tasks`}
                  />
                  <CarryOption
                    checked={carryAiSetup}
                    onChange={setCarryAiSetup}
                    disabled={
                      !previousMeeting.ai_model_override
                      && !previousMeeting.summary_preset_override
                      && !previousMeeting.summary_instructions.trim()
                    }
                    label="Reuse meeting-specific AI setup"
                  />
                </div>
              )}
            </div>
          )}

          <DialogField label="Agenda or purpose">
            <Input
              value={agenda}
              maxLength={2000}
              onChange={(event) => setAgenda(event.target.value)}
              placeholder="Review launch readiness and unblock pricing"
              className="mt-2"
            />
          </DialogField>

          <DialogField
            label="Participants"
            hint="Optional, separated by commas. Stored locally and used to improve owner attribution."
          >
            <Input
              value={participants}
              onChange={(event) => setParticipants(event.target.value)}
              placeholder="Alex, Jordan, Casey"
              className="mt-2"
            />
          </DialogField>

          <DialogField
            label="Meeting microphone"
            hint="System audio follows the current Windows default output device."
          >
            <div className="mt-2">
              <Select
                aria-label="Meeting microphone"
                options={microphoneOptions}
                value={selectedAudioDevice}
                onChange={(value) => void changeAudioDevice(value)}
                disabled={audioDevicesLoading || audioDevices.length === 0}
              />
            </div>
          </DialogField>

          <div className="rounded-[var(--radius-l)] border border-border bg-surface-2/40 p-3">
            <div className="flex gap-2.5">
              <ShieldCheck size={16} className="mt-0.5 shrink-0 text-success" />
              <div className="min-w-0 flex-1">
                <p className="text-xs font-medium text-text-primary">Local recording by default</p>
                <p className="mt-1 text-xs leading-relaxed text-text-muted">
                  Temporary audio chunks are deleted after local transcription. Meeting text and context
                  are sent only when OpenRouter intelligence runs.
                </p>
                <Button
                  size="sm"
                  variant="secondary"
                  className="mt-2.5"
                  disabled={testingAudio}
                  onClick={() => void runAudioTest()}
                  icon={testingAudio ? <Loader2 className="animate-spin" /> : <Headphones />}
                >
                  {testingAudio
                    ? "Listening for 2 seconds…"
                    : audioReadiness ? "Test again" : "Test meeting audio"}
                </Button>
              </div>
            </div>
            {audioReadiness && (
              <div className="mt-3 grid grid-cols-2 gap-2 border-t border-border pt-3">
                <AudioReadinessCard
                  label={selectedMicrophoneName}
                  source={audioReadiness.microphone}
                  silentHint="Speak to verify the level."
                />
                <AudioReadinessCard
                  label="Windows default output"
                  source={audioReadiness.system_audio}
                  silentHint="Play any sound to verify loopback."
                />
              </div>
            )}
          </div>
        </div>
      </Modal>

      <Modal
        open={providerOpen}
        onClose={() => setProviderOpen(false)}
        title="Meeting intelligence"
        description="Models, budget limits, privacy routing, and your OpenRouter connection."
        className="max-h-[calc(100vh-3rem)] overflow-y-auto"
      >
        <OpenRouterPanel />
      </Modal>

      <ConfirmDialog
        open={confirmDeleteTemplate}
        tone="danger"
        title="Delete meeting template"
        description={
          selectedTemplate
            ? `“${selectedTemplate.name}” will no longer be offered when starting a meeting. Meetings already started from it are untouched.`
            : undefined
        }
        confirmLabel="Delete template"
        onConfirm={() => void removeTemplate()}
        onCancel={() => setConfirmDeleteTemplate(false)}
      />

      <ConfirmDialog
        open={confirmPurge}
        tone="danger"
        title="Delete this meeting permanently"
        description="The transcript, notes, markers, and any remaining recovery audio are removed. This cannot be undone."
        confirmLabel="Delete permanently"
        onConfirm={purgeSelectedMeeting}
        onCancel={() => setConfirmPurge(false)}
      />
    </div>
  );
}

/** One labelled field in the start dialog: label above, optional hint below. */
function DialogField({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="block text-xs font-medium text-text-secondary">
      {label}
      {children}
      {hint && <span className="mt-1.5 block text-xs font-normal text-text-muted">{hint}</span>}
    </div>
  );
}

/** One row in the meeting list rail. */
function MeetingListRow({
  meeting,
  live,
  selected,
  status,
  favoritable,
  onOpen,
  onToggleFavorite,
}: {
  meeting: Meeting;
  live: boolean;
  selected: boolean;
  status: string;
  favoritable: boolean;
  onOpen: () => void;
  onToggleFavorite: () => void;
}) {
  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onOpen}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onOpen();
        }
      }}
      className={cn(
        "group relative w-full cursor-pointer px-3 py-2 text-left transition-colors duration-[var(--dur-2)] ease-out",
        "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-amber-500/60",
        selected ? "bg-surface-2/70" : "hover:bg-surface-2/40"
      )}
    >
      {selected && <span aria-hidden className="absolute inset-y-0 left-0 w-0.5 bg-amber-500" />}
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <div className={cn("truncate text-sm font-medium leading-5", selected ? "text-text-primary" : "text-text-secondary")}>
            {meeting.title}
          </div>
          <div className="mt-1 flex items-center gap-1.5 text-xs text-text-muted">
            <span className={cn("h-1.5 w-1.5 shrink-0 rounded-full", statusDot(status), live && "animate-pulse")} />
            <span className="tnum">
              {new Date(meeting.started_at).toLocaleDateString(undefined, { month: "short", day: "numeric" })}
            </span>
            <span aria-hidden className="text-text-muted/40">·</span>
            <span className="truncate capitalize">{status.replace(/_/g, " ")}</span>
          </div>
          {meeting.tags.length > 0 && (
            <div className="mt-1.5 flex gap-1 overflow-hidden">
              {meeting.tags.slice(0, 2).map((tag) => (
                <span key={tag} className="truncate rounded-[var(--radius-s)] bg-surface-3 px-1.5 py-0.5 text-xs text-text-muted">
                  #{tag}
                </span>
              ))}
            </div>
          )}
        </div>
        {favoritable && (
          <button
            onClick={(event) => { event.stopPropagation(); onToggleFavorite(); }}
            aria-label={meeting.is_favorite ? "Remove from favorites" : "Add to favorites"}
            className={cn(
              "rounded-[var(--radius-s)] p-1 transition-opacity duration-[var(--dur-2)] ease-out",
              meeting.is_favorite
                ? "text-amber-400"
                : "text-text-muted/50 opacity-0 group-hover:opacity-100 focus:opacity-100"
            )}
          >
            <Star size={13} fill={meeting.is_favorite ? "currentColor" : "none"} />
          </button>
        )}
      </div>
    </div>
  );
}

function MeetingsEmpty({ starting, disabled, onStart, onOptions, onProvider }: { starting: boolean; disabled: boolean; onStart: () => void; onOptions: () => void; onProvider: () => void }) {
  return <div className="flex h-full items-center justify-center p-8">
    <div className="max-w-lg text-center">
      <div className="mx-auto flex h-14 w-14 items-center justify-center rounded-2xl border border-amber-500/20 bg-amber-500/[0.08] text-amber-400"><Waves size={24} /></div>
      <h2 className="mt-5 text-xl font-semibold tracking-[-0.02em] text-text-primary">A quieter meeting copilot</h2>
      <p className="mx-auto mt-2 max-w-md text-sm leading-relaxed text-text-muted">Capture the conversation locally, keep your own scratch notes beside it, and generate decisions and action items when the call ends.</p>
      <div className="mt-6 flex flex-wrap justify-center gap-2">
        <Button variant="primary" icon={<Mic2 />} onClick={onStart} disabled={disabled}>{starting ? "Starting…" : "Start meeting"}</Button>
        <Button variant="secondary" icon={<ChevronDown />} onClick={onOptions} disabled={disabled}>Start with options</Button>
        <Button variant="ghost" icon={<Sparkles />} onClick={onProvider}>Connect AI</Button>
      </div>
      <div className="mt-8 grid grid-cols-3 gap-2 text-left">
        <Feature icon={ShieldCheck} title="Audio stays local" text="Deleted chunk-by-chunk after transcription." />
        <Feature icon={CalendarClock} title="Crash recoverable" text="Sealed chunks survive an interrupted call." />
        <Feature icon={ExternalLink} title="Cloud is explicit" text="Only text goes to your selected model." />
      </div>
    </div>
  </div>;
}

function TrashEmpty() {
  return <div className="flex h-full items-center justify-center">
    <EmptyState icon={<Trash2 />} title="Trash is empty" description="Meetings you move here remain recoverable until you delete them permanently." />
  </div>;
}

function TrashedMeeting({
  meeting,
  onRestore,
  onDeleteForever,
}: {
  meeting: Meeting;
  onRestore: () => void;
  onDeleteForever: () => void;
}) {
  return (
    <div className="flex h-full items-center justify-center p-8">
      <div className="w-full max-w-md text-center">
        <div className="mx-auto flex h-11 w-11 items-center justify-center rounded-full bg-surface-2 text-text-muted">
          <Trash2 size={19} />
        </div>
        <h2 className="mt-4 truncate text-lg font-semibold tracking-[-0.01em] text-text-primary">{meeting.title}</h2>
        <p className="tnum mt-1 text-xs text-text-muted">
          Moved to Trash {meeting.deleted_at ? new Date(meeting.deleted_at).toLocaleString() : "recently"}
        </p>
        {meeting.tags.length > 0 && (
          <div className="mt-3 flex flex-wrap justify-center gap-1">
            {meeting.tags.map((tag) => (
              <span key={tag} className="rounded-[var(--radius-s)] bg-surface-3 px-2 py-1 text-xs text-text-secondary">
                #{tag}
              </span>
            ))}
          </div>
        )}
        <div className="mt-6 flex justify-center gap-2">
          <Button variant="primary" onClick={onRestore} icon={<Undo2 />}>Restore meeting</Button>
          <Button variant="secondary" onClick={onDeleteForever} icon={<Trash2 />}>Delete forever</Button>
        </div>
        <p className="mt-4 text-xs leading-relaxed text-text-muted">
          Permanent deletion removes the transcript, notes, markers, and any remaining recovery audio.
        </p>
      </div>
    </div>
  );
}

function Feature({ icon: Icon, title, text }: { icon: typeof ShieldCheck; title: string; text: string }) {
  return (
    <div className="rounded-[var(--radius-l)] border border-border bg-surface-2/40 p-3">
      <Icon size={15} className="text-amber-400" />
      <div className="mt-2 text-xs font-medium text-text-primary">{title}</div>
      <p className="mt-1 text-xs leading-relaxed text-text-muted">{text}</p>
    </div>
  );
}

function CarryOption({
  checked,
  onChange,
  disabled,
  label,
  detail,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  disabled: boolean;
  label: string;
  detail?: string;
}) {
  return (
    <Checkbox
      checked={checked && !disabled}
      onChange={onChange}
      disabled={disabled}
      className="text-xs"
      label={
        <span className="text-xs leading-relaxed">
          {label}
          {detail && <span className="ml-1 text-text-muted">· {detail}</span>}
        </span>
      }
    />
  );
}

function AudioReadinessCard({
  label,
  source,
  silentHint,
}: {
  label: string;
  source: MeetingAudioReadiness["microphone"];
  silentHint: string;
}) {
  const state = !source.available
    ? "Unavailable"
    : source.signal_detected ? "Signal detected" : "Available · no signal yet";
  return (
    <div
      className={cn(
        "rounded-[var(--radius-m)] border p-2",
        source.available ? "border-success/15 bg-success/[0.05]" : "border-recording-500/20 bg-recording-500/[0.06]"
      )}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="min-w-0 truncate text-xs font-medium text-text-primary">{label}</span>
        <span className={cn("shrink-0 text-xs", source.available ? "text-success" : "text-recording-300")}>
          {state}
        </span>
      </div>
      {/* Teal, not amber: a level meter is telemetry, not an action or a warning.
          The width carries the "no signal yet" story on its own. */}
      <Progress
        className="mt-2"
        accent="teal"
        aria-label={`${label} input level`}
        value={Math.min(100, Math.max(source.available ? 4 : 0, source.peak_level * 100))}
      />
      <p className="mt-1.5 truncate text-xs text-text-muted" title={source.error ?? silentHint}>
        {source.error ?? (source.signal_detected ? "Capture path is working." : silentHint)}
      </p>
    </div>
  );
}
