import { invoke } from "@tauri-apps/api/core";
import { listen, type Event, type UnlistenFn } from "@tauri-apps/api/event";

const noopUnlisten: UnlistenFn = () => {};

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function listenSafe<T>(event: string, callback: (event: Event<T>) => void): Promise<UnlistenFn> {
  try {
    return await listen<T>(event, callback);
  } catch (error) {
    if (!isTauriRuntime()) {
      return noopUnlisten;
    }
    throw error;
  }
}

// Types matching Rust structs
export interface AudioDevice {
  id: string;
  name: string;
  is_default: boolean;
  sample_rate: number;
  channels: number;
}

/** Content-free production timing for one completed/superseded capture. Stage
 * offsets are milliseconds from the stop request, not from async task start. */
export interface PipelineTrace {
  generation: number;
  mode: "dictation" | "command";
  model: string | null;
  backend: "cpu" | "gpu" | null;
  audio_duration_ms: number;
  /** Backend capture claim to successful mic start; pre-claim OS hotkey
   * dispatch is outside this measurement. */
  claim_to_mic_live_ms: number | null;
  stop_received: number | null;
  audio_stopped: number | null;
  preview_drained: number | null;
  preprocess_done: number | null;
  asr_started: number | null;
  asr_done: number | null;
  llm_started: number | null;
  llm_done: number | null;
  output_started: number | null;
  output_done: number | null;
  stop_to_visible_delivery_ms: number | null;
  visible_delivery_kind: string | null;
  completed: number | null;
  outcome: string;
}

export interface ModelInfo {
  id: string;
  name: string;
  size_bytes: number;
  quantization: string;
  language_support: "english" | "multilingual";
  capability_tier: "entry" | "balanced" | "high";
  estimated_memory_mb: number;
  description: string;
  is_downloaded: boolean;
  path: string | null;
  bundled: boolean;
  recommended: boolean;
  family: "whisper" | "parakeet";
}

export interface DownloadProgress {
  model_id: string;
  downloaded_bytes: number;
  total_bytes: number;
  progress_percent: number;
  status: string;
}

export interface HardwareInfo {
  cpu_name: string;
  cpu_cores: number;
  ram_total_mb: number;
  gpu_name: string | null;
  gpu_vram_mb: number | null;
  compute_backend: "cpu" | "vulkan" | "cuda" | "metal" | "unknown";
  measured_asr_tier: "entry" | "balanced" | "high" | null;
  recommended_model: string;
}

export interface TranscriptionRecord {
  id: string;
  text: string;
  duration_ms: number;
  model_name: string;
  created_at: string;
  /** Original dictation before Structured Mode post-processing. */
  raw_transcript?: string | null;
}

export interface DictionaryEntry {
  id: string;
  phrase: string;
  replacement: string;
  is_enabled: boolean;
  created_at: string;
}

export interface Snippet {
  id: string;
  trigger: string;
  content: string;
  description: string | null;
  is_enabled: boolean;
  created_at: string;
}

export interface VocabularyEntry {
  id: string;
  word: string;
  is_enabled: boolean;
  created_at: string;
}

export interface HotkeyConfig {
  keys: number[];
  labels: string[];
}

export interface AppSettings {
  theme: string;
  /** Launch OmniVox automatically when the OS starts. */
  auto_start: boolean;
  output_mode: string;
  active_model_id: string | null;
  hotkey: HotkeyConfig | null;
  gpu_acceleration: boolean;
  active_context_mode_id: string | null;
  live_preview: boolean;
  noise_reduction: boolean;
  auto_switch_modes: boolean;
  voice_commands: boolean;
  command_send: boolean;
  /** Command Mode: hold Right Ctrl and speak a command (launch app, key chord, media). */
  command_mode: boolean;
  /**
   * Gate for the "Launch"/"open app" command action.  When false, voice
   * commands that would start an application are refused by the backend.
   * Default true.
   */
  launch_app_voice_commands_enabled: boolean;
  ship_mode: boolean;
  ghost_mode: boolean;
  writing_style: string;
  /** Remove filler words and stutter repeats during post-processing. */
  filler_removal: boolean;
  audio_ducking: boolean;
  ducking_amount: number;
  /** Send dictation through a local LLM and output a structured Markdown prompt. */
  structured_mode: boolean;
  /** Active LLM catalog ID for Structured Mode. */
  active_llm_model_id: string | null;
  /** Max seconds to wait for LLM inference before falling back to plain output. */
  llm_timeout_secs: number;
  /** Below this character count, Structured Mode is skipped. */
  structured_min_chars: number;
  /**
   * Voice-command gate for Structured Mode.  When true, the user must
   * end their dictation with the word "Voxify" before the LLM runs —
   * otherwise the transcription is output plain even with
   * `structured_mode` on.  Mirrors how `command_send` gates Ship Mode
   * behind the "send" word.
   */
  structured_voice_command: boolean;
  /**
   * Send dictation transcripts through the local S1-mini model to remove
   * fillers, resolve spoken self-corrections, fix punctuation/casing, and
   * format numbers/dates/emails. English only.
   */
  cleanup_mode: boolean;
  /** Active LLM catalog ID for AI Transcript Cleanup. */
  active_cleanup_model_id: string | null;
  /**
   * Read visible text from the foreground app and use it to bias Whisper
   * toward verbatim file paths, identifiers, and commands.  Local only —
   * captured text never leaves the device.
   */
  use_screen_context: boolean;
  /**
   * Also pass screen-context tokens into Structured Mode's LLM so it can
   * substitute phonetic guesses with verbatim screen text on multi-token
   * strings.  Independent of `use_screen_context` — the Whisper-only
   * path covers most cases on its own.
   */
  structured_use_screen_context: boolean;
  /** Persist completed dictations in the local SQLite history database. */
  history_enabled: boolean;
  /** Delete history older than this many days; 0 keeps it until manually deleted. */
  history_retention_days: number;
}

/** Field-level settings update. Use `patchSettings`, never merge and submit a
 * stale whole `AppSettings` object from a WebView. */
export type SettingsPatch = Partial<AppSettings>;

export interface SettingsSnapshot {
  revision: number;
  settings: AppSettings;
  /** The backend applied this patch to a newer snapshot than the caller had. */
  rebased: boolean;
}

export interface AppBinding {
  id: string;
  mode_id: string;
  process_name: string;
  created_at: string;
}

// Platform info
export interface PlatformInfo {
  os: string;
  needs_mic_permission: boolean;
  needs_accessibility_permission: boolean;
}
export const getPlatformInfo = () => invoke<PlatformInfo>("get_platform_info");
export const openMicSettings = () => invoke<void>("open_mic_settings");
export const openAccessibilitySettings = () =>
  invoke<void>("open_accessibility_settings");

// Audio commands
export const startRecording = () => invoke<void>("start_recording");
export const stopRecording = () => invoke<string>("stop_recording");
export const cancelRecording = () => invoke<void>("cancel_recording");
export const getAudioDevices = () => invoke<AudioDevice[]>("get_audio_devices");
export const getSelectedAudioDevice = () => invoke<string | null>("get_selected_audio_device");
export const setAudioDevice = (deviceId: string) =>
  invoke<void>("set_audio_device", { deviceId });
/** Main-window diagnostic query; returns the bounded, content-free trace ring. */
export const getPipelineTraces = () => invoke<PipelineTrace[]>("get_pipeline_traces");

// Model commands
export const listModels = () => invoke<ModelInfo[]>("list_models");
export const downloadModel = (modelId: string) =>
  invoke<void>("download_model", { modelId });
export const deleteModel = (modelId: string) =>
  invoke<void>("delete_model", { modelId });
export const getActiveModel = () => invoke<ModelInfo | null>("get_active_model");
export const setActiveModel = (modelId: string) =>
  invoke<void>("set_active_model", { modelId });
export const getHardwareInfo = () => invoke<HardwareInfo>("get_hardware_info");
export const getGpuSupport = () => invoke<boolean>("get_gpu_support");

// Dictionary commands
export const addDictionaryEntry = (phrase: string, replacement: string) =>
  invoke<DictionaryEntry>("add_dictionary_entry", { phrase, replacement });
export const updateDictionaryEntry = (id: string, phrase: string, replacement: string) =>
  invoke<void>("update_dictionary_entry", { id, phrase, replacement });
export const deleteDictionaryEntry = (id: string) =>
  invoke<void>("delete_dictionary_entry", { id });
export const listDictionaryEntries = () =>
  invoke<DictionaryEntry[]>("list_dictionary_entries");
export const addSnippet = (trigger: string, content: string, description?: string) =>
  invoke<Snippet>("add_snippet", { trigger, content, description: description ?? null });
export const updateSnippet = (id: string, trigger: string, content: string, description?: string) =>
  invoke<void>("update_snippet", { id, trigger, content, description: description ?? null });
export const deleteSnippet = (id: string) => invoke<void>("delete_snippet", { id });
export const listSnippets = () => invoke<Snippet[]>("list_snippets");

// Vocabulary commands
export const addVocabularyEntry = (word: string) =>
  invoke<VocabularyEntry>("add_vocabulary_entry", { word });
export const updateVocabularyEntry = (id: string, word: string) =>
  invoke<void>("update_vocabulary_entry", { id, word });
export const deleteVocabularyEntry = (id: string) =>
  invoke<void>("delete_vocabulary_entry", { id });
export const listVocabularyEntries = () =>
  invoke<VocabularyEntry[]>("list_vocabulary_entries");

// Voice command registry
export type TriggerScope = "anywhere" | "end_of_utterance";

export interface VoiceCommand {
  id: string;
  /** Spoken trigger, stored lowercased. */
  phrase: string;
  /** Built-in variant name ("NewLine") or a combo spec ("key:ctrl+shift+k"). */
  action: string;
  trigger_scope: TriggerScope;
  enabled: boolean;
  built_in: boolean;
  sort_order: number;
  created_at: string;
}

export const listVoiceCommands = () =>
  invoke<VoiceCommand[]>("list_voice_commands");
export const addVoiceCommand = (
  phrase: string,
  action: string,
  triggerScope: TriggerScope
) => invoke<VoiceCommand>("add_voice_command", { phrase, action, triggerScope });
export const updateVoiceCommand = (
  id: string,
  phrase: string,
  action: string,
  triggerScope: TriggerScope,
  enabled: boolean
) =>
  invoke<void>("update_voice_command", {
    id,
    phrase,
    action,
    triggerScope,
    enabled,
  });
export const deleteVoiceCommand = (id: string) =>
  invoke<void>("delete_voice_command", { id });
export const resetVoiceCommands = () =>
  invoke<VoiceCommand[]>("reset_voice_commands");

// History commands
export interface DictationStats {
  total_words: number;
  total_transcriptions: number;
  total_duration_ms: number;
}
export const getDictationStats = () =>
  invoke<DictationStats>("get_dictation_stats");

/** One lean row per transcription for the analytics page (no full text). */
export interface AnalyticsRecord {
  created_at: string;
  word_count: number;
  char_count: number;
  duration_ms: number;
  model_name: string;
}
export const getAnalyticsRecords = () =>
  invoke<AnalyticsRecord[]>("get_analytics_records");
export const searchHistory = (query: string, limit?: number, offset?: number) =>
  invoke<TranscriptionRecord[]>("search_history", {
    query,
    limit: limit ?? null,
    offset: offset ?? null,
  });
export const recentHistory = (limit?: number, offset?: number) =>
  invoke<TranscriptionRecord[]>("recent_history", {
    limit: limit ?? null,
    offset: offset ?? null,
  });
export const deleteHistoryRecord = (id: string) =>
  invoke<void>("delete_history_record", { id });
export const exportHistory = (format: string) =>
  invoke<string>("export_history", { format });

// Notes commands
export interface Note {
  id: string;
  title: string;
  content: string;
  created_at: string;
  updated_at: string;
}

export const addNote = (title: string, content: string) =>
  invoke<Note>("add_note", { title, content });
export const updateNote = (id: string, title: string, content: string) =>
  invoke<void>("update_note", { id, title, content });
export const deleteNote = (id: string) =>
  invoke<void>("delete_note", { id });
export const listNotes = () => invoke<Note[]>("list_notes");

// Settings commands
export const getSettings = () => invoke<AppSettings>("get_settings");
export const getSettingsSnapshot = () =>
  invoke<SettingsSnapshot>("get_settings_snapshot");
export const patchSettings = (
  patch: SettingsPatch,
  expectedRevision?: number
) => invoke<SettingsSnapshot>("patch_settings", { patch, expectedRevision });
/** Compatibility wrapper for older extensions/clients. New UI code uses
 * `patchSettings` to prevent stale whole-object writes. */
export const updateSettings = (settings: AppSettings) =>
  invoke<void>("update_settings", { settings });

// Hotkey commands
export const suspendHotkey = (suspended: boolean) =>
  invoke<void>("suspend_hotkey", { suspended });
export const feedHotkeyEvent = (vk: number, down: boolean) =>
  invoke<void>("feed_hotkey_event", { vk, down });
export const updateHotkey = (config: HotkeyConfig) =>
  invoke<void>("update_hotkey", { config });

// ── Command Mode ─────────────────────────────────────────────────────────
export interface CommandResult {
  generation: number;
  status: "done" | "error";
  summary: string;
}
export interface CommandConfirm {
  generation: number;
  /** Backend-issued id for this confirm, echoed back to confirm_command /
   *  cancel_command so a stale pill can't consume a newer command's confirm. */
  id: number;
  summary: string;
  /** Present when the pending action sends a typed message — shown in an
   *  editable textarea so a mishearing can be fixed before it sends. */
  editable_text?: string;
}

/** Execute the command currently awaiting confirmation.  `confirmId` is the id
 *  from the `command-confirm` event; pass `editedText` when the user revised
 *  the message in the confirm pill's textarea. */
export const confirmCommand = (confirmId: number, editedText?: string) =>
  invoke<void>("confirm_command", { confirmId, editedText: editedText ?? null });
/** Discard the command currently awaiting confirmation. */
export const cancelCommand = (confirmId: number) =>
  invoke<void>("cancel_command", { confirmId });

/** Resolved result of a "Test command" dry-run (no execution). */
export interface CommandTestResult {
  /** "matcher" (instant), "llm" (Qwen fallback), or "none". */
  tier: "matcher" | "llm" | "none";
  recognized: boolean;
  summary: string;
  duration_ms: number;
}
/** Dry-run an utterance through the command brain without executing it. */
export const testCommand = (utterance: string) =>
  invoke<CommandTestResult>("test_command", { utterance });

export const onCommandStateChange = (
  callback: (state: string, generation: number) => void
): Promise<UnlistenFn> =>
  listenGenerated<CommandStateChangePayload>("command-state-change", (payload) =>
    callback(payload.state, payload.generation)
  );

export const onCommandConfirm = (
  callback: (payload: CommandConfirm) => void
): Promise<UnlistenFn> =>
  listenGenerated<CommandConfirm>("command-confirm", callback);

export const onCommandResult = (
  callback: (payload: CommandResult) => void
): Promise<UnlistenFn> =>
  listenGenerated<CommandResult>("command-result", callback);

// Context mode types and commands
export interface ContextMode {
  id: string;
  name: string;
  description: string;
  icon: string;
  color: string;
  sort_order: number;
  is_builtin: boolean;
  created_at: string;
  updated_at: string;
  writing_style: string;
  /**
   * Structured Mode profile id for this mode ("agent-prompt", "email",
   * "notes-outline").  Empty = default agent-prompt.
   */
  structured_profile: string;
}

export const listContextModes = () => invoke<ContextMode[]>("list_context_modes");
export const getContextMode = (id: string) =>
  invoke<ContextMode>("get_context_mode", { id });
export const createContextMode = (
  name: string,
  description: string,
  icon: string,
  color: string,
  writingStyle: string,
  structuredProfile: string
) =>
  invoke<ContextMode>("create_context_mode", {
    name,
    description,
    icon,
    color,
    writingStyle,
    structuredProfile,
  });
export const updateContextMode = (
  id: string,
  name: string,
  description: string,
  icon: string,
  color: string,
  writingStyle: string,
  structuredProfile: string
) =>
  invoke<void>("update_context_mode", {
    id,
    name,
    description,
    icon,
    color,
    writingStyle,
    structuredProfile,
  });
export const deleteContextMode = (id: string) =>
  invoke<void>("delete_context_mode", { id });

// Mode-scoped dictionary/snippet commands (profile editor)
export const listModeDictionaryEntries = (modeId: string) =>
  invoke<DictionaryEntry[]>("list_mode_dictionary_entries", { modeId });
export const addModeDictionaryEntry = (modeId: string, phrase: string, replacement: string) =>
  invoke<DictionaryEntry>("add_mode_dictionary_entry", { modeId, phrase, replacement });
export const deleteModeDictionaryEntry = (id: string) =>
  invoke<void>("delete_mode_dictionary_entry", { id });
export const listModeSnippets = (modeId: string) =>
  invoke<Snippet[]>("list_mode_snippets", { modeId });
export const addModeSnippet = (modeId: string, trigger: string, content: string, description?: string) =>
  invoke<Snippet>("add_mode_snippet", { modeId, trigger, content, description: description ?? null });
export const deleteModeSnippet = (id: string) =>
  invoke<void>("delete_mode_snippet", { id });
export const listModeVocabularyEntries = (modeId: string) =>
  invoke<VocabularyEntry[]>("list_mode_vocabulary_entries", { modeId });
export const addModeVocabularyEntry = (modeId: string, word: string) =>
  invoke<VocabularyEntry>("add_mode_vocabulary_entry", { modeId, word });
export const deleteModeVocabularyEntry = (id: string) =>
  invoke<void>("delete_mode_vocabulary_entry", { id });
export const getActiveContextMode = () =>
  invoke<ContextMode | null>("get_active_context_mode");
export const setActiveContextMode = (id: string) =>
  invoke<void>("set_active_context_mode", { id });

// App binding commands
export const listAppBindings = (modeId: string) =>
  invoke<AppBinding[]>("list_app_bindings", { modeId });
export const addAppBinding = (modeId: string, processName: string) =>
  invoke<AppBinding>("add_app_binding", { modeId, processName });
export const deleteAppBinding = (id: string) =>
  invoke<void>("delete_app_binding", { id });

// Overlay commands
export const resizeOverlay = (width: number, height: number) =>
  invoke<void>("resize_overlay", { width, height });
export const showMainWindow = () => invoke<void>("show_main_window");

// Meeting Mode
export type MeetingStatus =
  | "recording"
  | "paused"
  | "transcribing"
  | "awaiting_summary"
  | "summarizing"
  | "ready"
  | "interrupted"
  | "failed"
  | "error";

export interface Meeting {
  id: string;
  title: string;
  status: MeetingStatus;
  source_app: string | null;
  started_at: string;
  ended_at: string | null;
  user_notes: string;
  summary_markdown: string;
  summary_json: string | null;
  summary_provider: string | null;
  summary_model: string | null;
  summary_cost: number | null;
  prompt_tokens: number | null;
  completion_tokens: number | null;
  is_favorite: boolean;
  tags: string[];
  completed_actions: string[];
  summary_stale: boolean;
  error: string | null;
  deleted_at: string | null;
  created_at: string;
  updated_at: string;
  ai_model_override: string | null;
  summary_preset_override: MeetingProviderSettings["summary_preset"] | null;
  summary_instructions: string;
  agenda: string;
  participants: string[];
  previous_meeting_id: string | null;
  actions_materialized: boolean;
}

export interface MeetingSegment {
  id: string;
  meeting_id: string;
  source: "mic" | "system";
  start_ms: number;
  end_ms: number;
  text: string;
  speaker_label: string | null;
  created_at: string;
}

export interface MeetingMarker {
  id: string;
  meeting_id: string;
  at_ms: number;
  label: string;
  created_at: string;
}

export interface MeetingActionItem {
  id: string;
  meeting_id: string;
  task: string;
  owner: string | null;
  due: string | null;
  segment_refs: string[];
  origin: "ai" | "manual" | string;
  completed: boolean;
  created_at: string;
  updated_at: string;
}

export interface MeetingQuestionAnswer {
  id: string;
  meeting_id: string;
  question: string;
  answer: string;
  segment_refs: string[];
  provider: string;
  model: string;
  cost: number;
  prompt_tokens: number;
  completion_tokens: number;
  context_segments: number;
  total_segments: number;
  created_at: string;
}

export interface MeetingSummaryVersion {
  id: string;
  meeting_id: string;
  markdown: string;
  summary_json: string | null;
  provider: string | null;
  model: string | null;
  cost: number | null;
  prompt_tokens: number | null;
  completion_tokens: number | null;
  origin: "ai" | "manual" | "restored" | string;
  created_at: string;
}

export interface MeetingRecoveryItem {
  meeting_id: string;
  title: string;
  status: MeetingStatus;
  pending_chunks: number;
  processing_chunks: number;
  failed_chunks: number;
  recoverable_failed_chunks: number;
  completed_chunks: number;
  has_summary: boolean;
  error: string | null;
  updated_at: string;
}

export interface MeetingTranscriptionProgress {
  total: number;
  completed: number;
  pending: number;
  processing: number;
  failed: number;
}

export type MeetingDetail = Meeting & {
  segments: MeetingSegment[];
  markers: MeetingMarker[];
  action_items: MeetingActionItem[];
  questions: MeetingQuestionAnswer[];
  transcription: MeetingTranscriptionProgress;
};

export interface MeetingRuntimeState {
  meeting_id: string | null;
  status: "idle" | "recording" | "paused" | "transcribing";
  elapsed_ms: number;
  mic_level: number;
  system_level: number;
  mic_signal_detected: boolean;
  system_signal_detected: boolean;
  capture_warning: string | null;
}

export interface MeetingAudioSourceReadiness {
  available: boolean;
  signal_detected: boolean;
  peak_level: number;
  error: string | null;
}

export interface MeetingAudioReadiness {
  microphone: MeetingAudioSourceReadiness;
  system_audio: MeetingAudioSourceReadiness;
}

export interface MeetingProviderSettings {
  provider: string;
  model: string;
  monthly_budget: number;
  per_meeting_budget: number;
  max_prompt_price: number;
  max_completion_price: number;
  zdr_only: boolean;
  deny_data_collection: boolean;
  auto_suggest: boolean;
  auto_summarize: boolean;
  summary_preset: "general" | "executive" | "one_on_one" | "sales" | "interview" | "standup";
  custom_instructions: string;
  transcription_mode: "after_meeting" | "live";
  routing_preference: "balanced" | "price" | "throughput" | "latency";
}

export interface MeetingUsageStats {
  month_spend: number;
  request_count: number;
  prompt_tokens: number;
  completion_tokens: number;
}

export interface MeetingAiUsageRecord {
  id: string;
  meeting_id: string | null;
  meeting_title: string | null;
  request_kind: "summary" | "question" | "legacy" | string;
  provider: string;
  model: string;
  cost: number;
  prompt_tokens: number;
  completion_tokens: number;
  created_at: string;
}

export interface MeetingTemplate {
  id: string;
  name: string;
  title: string;
  agenda: string;
  participants: string[];
  ai_model_override: string | null;
  summary_preset_override: MeetingProviderSettings["summary_preset"] | null;
  summary_instructions: string;
  created_at: string;
  updated_at: string;
}

export interface MeetingSummaryEstimate {
  model: string;
  model_name: string;
  estimated_cost: number;
  estimated_prompt_tokens: number;
  max_completion_tokens: number;
  required_context_tokens: number;
  context_length: number;
  prompt_price_million: number;
  completion_price_million: number;
  request_price: number;
  month_spend: number;
  meeting_spend: number;
  monthly_budget: number;
  per_meeting_budget: number;
  allowed: boolean;
  blocking_reason: string | null;
}

export interface MeetingExportResult {
  path: string;
  file_name: string;
}

export interface ProviderStatus {
  key_present: boolean;
  connected: boolean;
  label: string | null;
  limit: number | null;
  limit_remaining: number | null;
  limit_reset: string | null;
  usage: number | null;
  usage_daily: number | null;
  usage_weekly: number | null;
  usage_monthly: number | null;
  is_free_tier: boolean | null;
  is_management_key: boolean | null;
  expires_at: string | null;
  error: string | null;
}

export interface OpenRouterModel {
  id: string;
  name: string;
  description: string;
  context_length: number;
  prompt_price_million: number;
  completion_price_million: number;
  request_price: number;
  supports_structured_output: boolean;
  expiration_date: string | null;
  /** OpenRouter's Artificial Analysis intelligence index when available. */
  intelligence_index: number | null;
}

export const meetingStart = (title: string, sourceApp?: string, previousMeetingId?: string) =>
  invoke<Meeting>("meeting_start", {
    title,
    sourceApp: sourceApp ?? null,
    previousMeetingId: previousMeetingId ?? null,
  });
export const meetingPause = () => invoke<void>("meeting_pause");
export const meetingResume = () => invoke<void>("meeting_resume");
export const meetingStop = () => invoke<string>("meeting_stop");
export const getMeetingState = () => invoke<MeetingRuntimeState>("meeting_state");
export const testMeetingAudio = () => invoke<MeetingAudioReadiness>("test_meeting_audio");
export const listMeetings = () => isTauriRuntime() ? invoke<Meeting[]>("list_meetings") : Promise.resolve([]);
export const listMeetingRecoveryItems = () => invoke<MeetingRecoveryItem[]>("list_meeting_recovery_items");
export const listMeetingTemplates = () => invoke<MeetingTemplate[]>("list_meeting_templates");
export const saveMeetingAsTemplate = (meetingId: string, name: string) =>
  invoke<MeetingTemplate>("save_meeting_as_template", { meetingId, name });
export const deleteMeetingTemplate = (id: string) =>
  invoke<void>("delete_meeting_template", { id });
export const listDeletedMeetings = () => isTauriRuntime() ? invoke<Meeting[]>("list_deleted_meetings") : Promise.resolve([]);
export const searchMeetings = (query: string) => isTauriRuntime() ? invoke<Meeting[]>("search_meetings", { query }) : Promise.resolve([]);
export const searchDeletedMeetings = (query: string) => isTauriRuntime() ? invoke<Meeting[]>("search_deleted_meetings", { query }) : Promise.resolve([]);
export const getMeeting = (id: string) => invoke<MeetingDetail | null>("get_meeting", { id });
export const updateMeetingNotes = (id: string, notes: string) =>
  invoke<void>("update_meeting_notes", { id, notes });
export const updateMeetingAiOptions = (
  id: string,
  model: string | null,
  preset: MeetingProviderSettings["summary_preset"] | null,
  instructions: string,
) => invoke<void>("update_meeting_ai_options", { id, model, preset, instructions });
export const updateMeetingMetadata = (
  id: string,
  agenda: string | null,
  participants: string[] | null,
) => invoke<string[]>("update_meeting_metadata", { id, agenda, participants });
export const updateMeetingTitle = (id: string, title: string) =>
  invoke<void>("update_meeting_title", { id, title });
export const setMeetingFavorite = (id: string, favorite: boolean) =>
  invoke<void>("set_meeting_favorite", { id, favorite });
export const setMeetingTags = (id: string, tags: string[]) =>
  invoke<string[]>("set_meeting_tags", { id, tags });
export const updateMeetingSegment = (id: string, meetingId: string, text: string) =>
  invoke<void>("update_meeting_segment", { id, meetingId, text });
export const updateMeetingSegmentSpeaker = (id: string, meetingId: string, label: string, applyToSource = false) =>
  invoke<void>("update_meeting_segment_speaker", { id, meetingId, label, applyToSource });
export const setMeetingActionCompleted = (id: string, task: string, completed: boolean) =>
  invoke<string[]>("set_meeting_action_completed", { id, task, completed });
export const listMeetingActionItems = (meetingId?: string) =>
  invoke<MeetingActionItem[]>("list_meeting_action_items", { meetingId: meetingId ?? null });
export const addMeetingActionItem = (meetingId: string, task: string, owner?: string, due?: string) =>
  invoke<MeetingActionItem>("add_meeting_action_item", { meetingId, task, owner: owner ?? null, due: due ?? null });
export const updateMeetingActionItem = (id: string, meetingId: string, task: string, owner?: string, due?: string) =>
  invoke<MeetingActionItem>("update_meeting_action_item", { id, meetingId, task, owner: owner ?? null, due: due ?? null });
export const setMeetingActionItemCompleted = (id: string, meetingId: string, completed: boolean) =>
  invoke<MeetingActionItem>("set_meeting_action_item_completed", { id, meetingId, completed });
export const deleteMeetingActionItem = (id: string, meetingId: string) =>
  invoke<void>("delete_meeting_action_item", { id, meetingId });
export const addMeetingMarker = (id: string, atMs?: number, label?: string) =>
  invoke<MeetingMarker>("add_meeting_marker", { id, atMs: atMs ?? null, label: label ?? null });
export const updateMeetingMarker = (id: string, meetingId: string, label: string) =>
  invoke<void>("update_meeting_marker", { id, meetingId, label });
export const deleteMeetingMarker = (id: string, meetingId: string) =>
  invoke<void>("delete_meeting_marker", { id, meetingId });
export const updateMeetingSummary = (id: string, markdown: string) =>
  invoke<void>("update_meeting_summary", { id, markdown });
export const listMeetingSummaryVersions = (meetingId: string) =>
  invoke<MeetingSummaryVersion[]>("list_meeting_summary_versions", { meetingId });
export const restoreMeetingSummaryVersion = (id: string, meetingId: string) =>
  invoke<MeetingSummaryVersion>("restore_meeting_summary_version", { id, meetingId });
export const deleteMeeting = (id: string) => invoke<void>("delete_meeting", { id });
export const restoreMeeting = (id: string) => invoke<void>("restore_meeting", { id });
export const permanentlyDeleteMeeting = (id: string) =>
  invoke<void>("permanently_delete_meeting", { id });
export const estimateMeetingSummary = (id: string, model?: string) =>
  invoke<MeetingSummaryEstimate>("estimate_meeting_summary", { id, model: model ?? null });
export const askMeetingQuestion = (id: string, question: string, model?: string) =>
  invoke<MeetingQuestionAnswer>("ask_meeting_question", { id, question, model: model ?? null });
export const deleteMeetingQuestion = (id: string, meetingId: string) =>
  invoke<void>("delete_meeting_question", { id, meetingId });
export const summarizeMeeting = (id: string, model?: string) =>
  invoke<void>("summarize_meeting", { id, model: model ?? null });
export const retryMeetingTranscription = (id: string) =>
  invoke<number>("retry_meeting_transcription", { id });
export const getMeetingProviderSettings = () =>
  invoke<MeetingProviderSettings>("get_meeting_provider_settings");
export const getMeetingUsageStats = () => invoke<MeetingUsageStats>("get_meeting_usage_stats");
export const listMeetingAiUsage = (limit = 200) =>
  invoke<MeetingAiUsageRecord[]>("list_meeting_ai_usage", { limit });
export const saveMeetingProviderSettings = (settings: MeetingProviderSettings) =>
  invoke<void>("save_meeting_provider_settings", { settings });
export const saveOpenRouterKey = (key: string) =>
  invoke<ProviderStatus>("save_openrouter_key", { key });
export const removeOpenRouterKey = () => invoke<void>("remove_openrouter_key");
export const getOpenRouterStatus = () => invoke<ProviderStatus>("get_openrouter_status");
export const listOpenRouterModels = () => invoke<OpenRouterModel[]>("list_openrouter_models");
export const openOpenRouterCredits = () => invoke<void>("open_openrouter_credits");
export const openOpenRouterKeys = () => invoke<void>("open_openrouter_keys");
export type MeetingExportFormat = "notes" | "markdown" | "json" | "webvtt" | "srt";
export const exportMeeting = (id: string, format: MeetingExportFormat) =>
  invoke<MeetingExportResult>("export_meeting", { id, format });
export const revealMeetingExport = (path: string) =>
  invoke<void>("reveal_meeting_export", { path });
export const openMeetingDrawer = (meetingId?: string) =>
  invoke<void>("open_meeting_drawer", { meetingId: meetingId ?? null });
export const closeMeetingDrawer = () => invoke<void>("close_meeting_drawer");
export const dismissMeetingWidget = () => invoke<void>("dismiss_meeting_widget");
export const dismissMeetingSuggestion = () => invoke<void>("dismiss_meeting_suggestion");

export const onMeetingState = (callback: (payload: MeetingRuntimeState) => void) =>
  listenSafe<MeetingRuntimeState>("meeting-state", (event) => callback(event.payload));
export const onMeetingUpdated = (callback: (meetingId: string) => void) =>
  listenSafe<string>("meeting-updated", (event) => callback(event.payload));
export const onMeetingSummaryReady = (callback: (meetingId: string) => void) =>
  listenSafe<string>("meeting-summary-ready", (event) => callback(event.payload));
export const onMeetingError = (callback: (reason: string) => void) =>
  listenSafe<string>("meeting-error", (event) => callback(event.payload));
export const onMeetingSelect = (callback: (meetingId: string) => void) =>
  listenSafe<string>("meeting-select", (event) => callback(event.payload));
export interface MeetingSuggestionPayload {
  app: string;
  suggested_title: string;
}

export const onMeetingSuggestion = (callback: (payload: MeetingSuggestionPayload) => void) =>
  listenSafe<MeetingSuggestionPayload>("meeting-suggestion", (event) => callback(event.payload));

// ── Structured Mode / LLM ────────────────────────────────────────────────
export interface LlmModelInfo {
  id: string;
  name: string;
  size_bytes: number;
  quantization: string;
  family: string;
  parameter_count_millions: number;
  language_support: "multilingual";
  capability_tier: "fast" | "quality";
  estimated_memory_mb: number;
  context_length: number;
  description: string;
  huggingface_repo: string;
  huggingface_file: string;
  is_downloaded: boolean;
  path: string | null;
  is_default: boolean;
  /** "structured" powers Structured Mode / Command Mode; "cleanup" powers AI Transcript Cleanup. */
  purpose: "structured" | "cleanup";
}

export interface LlmDownloadProgress {
  model_id: string;
  downloaded_bytes: number;
  total_bytes: number;
  progress_percent: number;
  status: "downloading" | "completed" | "cancelled" | "failed";
}

export type Urgency = "low" | "normal" | "high";

export interface SlotExtraction {
  goal: string;
  /**
   * Background/situational statements from the dictation.  Renders as
   * `## Context`.  (Present in the Rust schema since v0.2.x; was missing
   * from this mirror, silently dropping the slot in any TS consumer.)
   * Absent on the wire when empty — Rust skip-serializes empty slots.
   */
  context?: string[];
  constraints: string[];
  files: string[];
  urgency?: Urgency | null;
  /**
   * Positive user-flow / acceptance-criteria statements describing how the
   * end experience should work.  Renders as `## Expected Behavior`.
   * Populated mainly for IMPLEMENTATION intents.
   */
  expected_behavior: string[];
  /**
   * Open questions the user wants explored / investigated / answered.
   * Renders as `## Open Questions`.  Populated for EXPLORATION / research
   * intents where the dictation isn't asking for an immediate build.
   */
  questions: string[];
  /**
   * Alternatives the user is weighing or comparing.  Renders as
   * `## Options`.  Populated for ADVICE / decision intents.
   */
  options: string[];
}

export interface StructuredOutputPayload extends GenerationTaggedPayload {
  /** One-time backend capability bound to this capture's HWND/PID target. */
  binding_id?: string | null;
  markdown: string;
  /**
   * Profile-specific slot object.  `SlotExtraction`-shaped for the default
   * agent-prompt profile; the email / notes-outline profiles emit their own
   * shapes (none of these keys), so every field is effectively optional.
   */
  slots: Partial<SlotExtraction>;
  raw_transcript: string;
  /**
   * Characters dropped from the LLM input because the dictation exceeded
   * the structured input cap.  0 when nothing was truncated.  The full
   * dictation is always preserved in `raw_transcript`.
   */
  truncated_chars: number;
}

export const listLlmModels = () => invoke<LlmModelInfo[]>("list_llm_models");
export const downloadLlmModel = (modelId: string) =>
  invoke<void>("download_llm_model", { modelId });
export const deleteLlmModel = (modelId: string) =>
  invoke<void>("delete_llm_model", { modelId });
export const getActiveLlmModel = () =>
  invoke<LlmModelInfo | null>("get_active_llm_model");
export const setActiveLlmModel = (modelId: string) =>
  invoke<void>("set_active_llm_model", { modelId });
export const llmTestExtract = (text?: string) =>
  invoke<string>("llm_test_extract", { text: text ?? null });

export interface LlmCleanupResult {
  output: string;
  duration_ms: number;
}
export const llmTestCleanup = (text: string) =>
  invoke<LlmCleanupResult>("llm_test_cleanup", { text });
export const pasteStructuredOutput = (
  markdown: string,
  bindingId: string,
  generation: number
) => invoke<void>("paste_structured_output", { markdown, bindingId, generation });
export const discardStructuredOutput = (bindingId: string, generation: number) =>
  invoke<void>("discard_structured_output", { bindingId, generation });
export const setStructuredPanelActive = (active: boolean) =>
  invoke<void>("set_structured_panel_active", { active });
// The overlay is non-activatable by default (it must never steal the
// foreground from the dictation target); surfaces that need typing flip this
// on while mounted.
export const setOverlayFocusable = (focusable: boolean) =>
  invoke<void>("set_overlay_focusable", { focusable });

/** Mirrors Rust `llm::diaglog::ExtractionRecord`. */
export interface LlmExtractionRecord {
  /** RFC 3339 UTC. */
  timestamp: string;
  duration_ms: number;
  /** Chars actually sent to the LLM (post-truncation). */
  input_chars: number;
  /** Chars dropped by the input cap (0 = nothing truncated). */
  truncated_chars: number;
  /** Rendered markdown length; 0 when the extraction failed. */
  output_chars: number;
  /** "ok", or the degradation reason shown to the user. */
  outcome: string;
}

/** Recent structured-mode extraction attempts, newest first. */
export const getLlmDiagnostics = () =>
  invoke<LlmExtractionRecord[]>("get_llm_diagnostics");

export const onLlmDownloadProgress = (
  callback: (progress: LlmDownloadProgress) => void
): Promise<UnlistenFn> =>
  listenSafe<LlmDownloadProgress>("llm-download-progress", (e) =>
    callback(e.payload)
  );

export const onLlmModelLoaded = (
  callback: (modelId: string) => void
): Promise<UnlistenFn> =>
  listenSafe<string>("llm-model-loaded", (e) => callback(e.payload));

/**
 * Structured-Mode LLM lifecycle: "loading" while the GGUF loads,
 * "ready" once usable, "error: …" on a failed load.  Lets the overlay
 * explain why a first structured dictation is slow instead of
 * appearing hung.
 */
export const onLlmStatus = (
  callback: (status: string) => void
): Promise<UnlistenFn> =>
  listenSafe<string>("llm-status", (e) => callback(e.payload));

export const onStructuredOutputReady = (
  callback: (payload: StructuredOutputPayload) => void
): Promise<UnlistenFn> =>
  listenGenerated<StructuredOutputPayload>("structured-output-ready", callback);

export const onStructuredModeDegraded = (
  callback: (reason: string) => void
): Promise<UnlistenFn> =>
  listenSafe<string>("structured-mode-degraded", (e) => callback(e.payload));

// Fired when a model was loaded on CPU because the GPU load failed (even
// after a retry). Without this the fallback is invisible: the UI shows the
// right model, it just transcribes several times slower.
export const onWhisperGpuFallback = (
  callback: (message: string) => void
): Promise<UnlistenFn> =>
  listenSafe<string>("whisper-gpu-fallback", (e) => callback(e.payload));

export interface LlmBackendFallback {
  model_id: string;
  backend: { kind: "gpu_partial"; layers: number } | { kind: "cpu_fallback" };
  duration_ms: number;
}

// Structured and Command Mode share this runner. Surface a partial/CPU
// fallback explicitly so an unexpectedly slow model never looks healthy.
export const onLlmGpuFallback = (
  callback: (outcome: LlmBackendFallback) => void
): Promise<UnlistenFn> =>
  listenSafe<LlmBackendFallback>("llm-gpu-fallback", (e) => callback(e.payload));

// Fired at most once per session when GPU acceleration is active but the only
// Vulkan device is an integrated GPU — model memory then sits in system RAM.
// Seen when a dedicated GPU loses its Vulkan driver registration after a
// graphics driver update.
export const onGpuEnvironmentWarning = (
  callback: (message: string) => void
): Promise<UnlistenFn> =>
  listenSafe<string>("gpu-environment-warning", (e) => callback(e.payload));

// ── Scratchpad ──────────────────────────────────────────────────────────────
export interface ScratchpadEntry {
  id: string;
  pad_id: string;
  content: string;
  created_at: string;
}
export interface ScratchpadPad {
  id: string;
  name: string;
  position: number;
  /** Newest-first. */
  entries: ScratchpadEntry[];
}
export type ScratchpadVariant = "note" | "entries" | "pads";
export interface ScratchpadData {
  note: string;
  pads: ScratchpadPad[];
  active_pad_id: string;
  variant: ScratchpadVariant;
}

export const openScratchpad = () => invoke<void>("open_scratchpad");
export const closeScratchpad = () => invoke<void>("close_scratchpad");
export const scratchpadGet = () => invoke<ScratchpadData>("scratchpad_get");
export const scratchpadSetNote = (content: string) =>
  invoke<void>("scratchpad_set_note", { content });
export const scratchpadAddEntry = (padId: string | null, content: string) =>
  invoke<ScratchpadEntry>("scratchpad_add_entry", { padId, content });
export const scratchpadDeleteEntry = (id: string) =>
  invoke<void>("scratchpad_delete_entry", { id });
export const scratchpadClearPad = (padId: string | null) =>
  invoke<void>("scratchpad_clear_pad", { padId });
export const scratchpadSetVariant = (variant: ScratchpadVariant) =>
  invoke<void>("scratchpad_set_variant", { variant });
export const saveScratchpadPosition = (x: number, y: number) =>
  invoke<void>("save_scratchpad_position", { x, y });
export const saveScratchpadSize = (w: number, h: number) =>
  invoke<void>("save_scratchpad_size", { w, h });
export const setScratchpadCapture = (on: boolean) =>
  invoke<void>("set_scratchpad_capture", { on });
export const scratchpadGetCapture = () => invoke<boolean>("scratchpad_get_capture");

// Event listeners

/**
 * Every capture-owned event is tagged with the capture generation allocated by
 * the backend. Events for a lower generation arrived late from an older
 * capture and must not be allowed to roll a WebView back to stale UI.
 *
 * A single gate is intentionally shared by dictation and Command Mode events:
 * they are mutually exclusive views of the same backend capture lifecycle.
 * Equality is accepted because one generation emits several related events.
 */
export interface GenerationTaggedPayload {
  generation: number;
}

export function createGenerationGate(initialGeneration = -1) {
  let latestGeneration = initialGeneration;
  return {
    accept(generation: number): boolean {
      if (!Number.isSafeInteger(generation) || generation < 0) return false;
      if (generation < latestGeneration) return false;
      latestGeneration = generation;
      return true;
    },
    latest: () => latestGeneration,
  };
}

const captureGenerationGate = createGenerationGate();

/**
 * Completion deliveries are not lifecycle state. An older dictation may
 * legitimately finish after a newer capture starts, so comparing it with the
 * latest lifecycle generation would discard user text. This gate validates
 * generations and delivers each completion exactly once, while the monotonic
 * `captureGenerationGate` above continues to reject stale state/meter events.
 */
export function createCompletionGenerationGate() {
  const delivered = new Set<number>();
  return {
    accept(generation: number): boolean {
      if (!Number.isSafeInteger(generation) || generation < 0) return false;
      if (delivered.has(generation)) return false;
      delivered.add(generation);
      return true;
    },
    hasDelivered: (generation: number) => delivered.has(generation),
  };
}

const RECORDING_PHASE_RANK: Record<string, number> = {
  recording: 0,
  processing: 1,
  structuring: 2,
  idle: 3,
  error: 3,
};

/**
 * Some consumers own work for more than one overlapping capture and therefore
 * need each generation's terminal event even after a newer capture starts.
 * Track progression independently per generation; the ordinary UI listener
 * below remains monotonic so late events still cannot roll global state back.
 */
export function createPerGenerationLifecycleGate() {
  const phases = new Map<number, number>();
  return {
    accept(generation: number, state: string): boolean {
      if (!Number.isSafeInteger(generation) || generation < 0) return false;
      const next = RECORDING_PHASE_RANK[state];
      if (next === undefined) return false;
      const previous = phases.get(generation);
      if (previous !== undefined && next <= previous) return false;
      phases.set(generation, next);
      return true;
    },
  };
}

function listenGenerated<T extends GenerationTaggedPayload>(
  event: string,
  callback: (payload: T) => void
): Promise<UnlistenFn> {
  return listenSafe<T>(event, (e) => {
    if (captureGenerationGate.accept(e.payload.generation)) {
      callback(e.payload);
    }
  });
}

export interface RecordingStateChangePayload extends GenerationTaggedPayload {
  state: string;
}

export interface AudioLevelPayload extends GenerationTaggedPayload {
  level: number;
}

export interface TranscriptionTextPayload extends GenerationTaggedPayload {
  text: string;
}

export interface CommandStateChangePayload extends GenerationTaggedPayload {
  state: string;
}

export const onRecordingStateChange = (
  callback: (status: string, generation: number) => void
): Promise<UnlistenFn> =>
  listenGenerated<RecordingStateChangePayload>("recording-state-change", (payload) =>
    callback(payload.state, payload.generation)
  );

/** Per-generation lifecycle delivery for owners of overlapping capture work. */
export const onRecordingLifecycle = (
  callback: (status: string, generation: number) => void
): Promise<UnlistenFn> => {
  const gate = createPerGenerationLifecycleGate();
  return listenSafe<RecordingStateChangePayload>("recording-state-change", (event) => {
    if (gate.accept(event.payload.generation, event.payload.state)) {
      callback(event.payload.state, event.payload.generation);
    }
  });
};

export const onAudioLevel = (
  callback: (level: number, generation: number) => void
): Promise<UnlistenFn> =>
  listenGenerated<AudioLevelPayload>("audio-level", (payload) =>
    callback(payload.level, payload.generation)
  );

export const onDownloadProgress = (
  callback: (progress: DownloadProgress) => void
): Promise<UnlistenFn> =>
  listenSafe<DownloadProgress>("download-progress", (e) => callback(e.payload));

export const onTranscriptionResult = (
  callback: (text: string, generation: number) => void
): Promise<UnlistenFn> =>
  listenGenerated<TranscriptionTextPayload>("transcription-result", (payload) =>
    callback(payload.text, payload.generation)
  );

/** Durable completion delivery for capture-owned editors. */
export const onTranscriptionCompletion = (
  callback: (text: string, generation: number) => void
): Promise<UnlistenFn> => {
  const completionGate = createCompletionGenerationGate();
  return listenSafe<TranscriptionTextPayload>("transcription-result", (event) => {
    if (completionGate.accept(event.payload.generation)) {
      callback(event.payload.text, event.payload.generation);
    }
  });
};

/** Fired after retention or privacy policy removes saved transcripts. */
export const onHistoryChanged = (callback: (deleted: number) => void): Promise<UnlistenFn> =>
  listenSafe<number>("history-changed", (event) => callback(event.payload));

/** Fired when a background retention/privacy purge could not complete. */
export const onHistoryCleanupError = (callback: (reason: string) => void): Promise<UnlistenFn> =>
  listenSafe<string>("history-cleanup-error", (event) => callback(event.payload));

export interface DictationInsertPayload extends GenerationTaggedPayload {
  text: string;
  /** OmniVox window label the dictation was aimed at, decided by the backend
   *  from the HWND snapshotted at record start: "main" | "scratchpad". */
  target: string;
}

/**
 * Fired when a dictation was aimed at one of OmniVox's own windows.  A
 * synthetic Ctrl+V doesn't reliably land in our WebView2 inputs, so the
 * backend hands the text here, tagged with the TARGET window label so each
 * window can deliver (caret-insert / append) or stand down deterministically.
 */
export const onDictationInsert = (
  callback: (payload: DictationInsertPayload) => void
): Promise<UnlistenFn> => {
  // Per-listener deduplication preserves normal event fan-out when more than
  // one consumer is mounted in a WebView.
  const completionGate = createCompletionGenerationGate();
  return listenSafe<DictationInsertPayload>("dictation-insert", (event) => {
    if (completionGate.accept(event.payload.generation)) {
      callback(event.payload);
    }
  });
};

/** Fired when a voice command changed the scratchpad's stored content (e.g.
 *  "clear the scratchpad") so an open pad reloads from the DB. */
export const onScratchpadRefresh = (callback: () => void): Promise<UnlistenFn> =>
  listenSafe<void>("scratchpad-refresh", () => callback());

export const onModelLoaded = (
  callback: (modelId: string) => void
): Promise<UnlistenFn> => listenSafe<string>("model-loaded", (e) => callback(e.payload));

export interface RecordingError extends GenerationTaggedPayload {
  state: string;
  code: string;
  message: string;
}

export const onRecordingError = (
  callback: (error: RecordingError) => void
): Promise<UnlistenFn> =>
  listenGenerated<RecordingError>("recording-error", callback);

export const onContextModeChanged = (
  callback: (mode: { id: string; name: string; icon: string; color: string }) => void
): Promise<UnlistenFn> =>
  listenSafe("context-mode-changed", (e) => callback(e.payload as { id: string; name: string; icon: string; color: string }));

export const onTranscriptionPreview = (
  callback: (text: string, generation: number) => void
): Promise<UnlistenFn> =>
  listenGenerated<TranscriptionTextPayload>("transcription-preview", (payload) =>
    callback(payload.text, payload.generation)
  );

export const onSettingsChanged = (
  callback: (settings: AppSettings) => void
): Promise<UnlistenFn> =>
  listenSafe<AppSettings>("settings-changed", (e) => callback(e.payload));
