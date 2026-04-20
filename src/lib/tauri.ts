import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// Types matching Rust structs
export interface AudioDevice {
  id: string;
  name: string;
  is_default: boolean;
  sample_rate: number;
  channels: number;
}

export interface ModelInfo {
  id: string;
  name: string;
  size_bytes: number;
  quantization: string;
  description: string;
  is_downloaded: boolean;
  path: string | null;
  bundled: boolean;
  recommended: boolean;
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
  language: string;
  auto_start: boolean;
  minimize_to_tray: boolean;
  output_mode: string;
  sample_rate: number;
  active_model_id: string | null;
  hotkey: HotkeyConfig | null;
  gpu_acceleration: boolean;
  active_context_mode_id: string | null;
  live_preview: boolean;
  noise_reduction: boolean;
  auto_switch_modes: boolean;
  voice_commands: boolean;
  command_send: boolean;
  ship_mode: boolean;
  ghost_mode: boolean;
  writing_style: string;
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
export const setAudioDevice = (deviceId: string) =>
  invoke<void>("set_audio_device", { deviceId });

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

// History commands
export interface DictationStats {
  total_words: number;
  total_transcriptions: number;
  total_duration_ms: number;
}
export const getDictationStats = () =>
  invoke<DictationStats>("get_dictation_stats");
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
export const updateSettings = (settings: AppSettings) =>
  invoke<void>("update_settings", { settings });

// Hotkey commands
export const suspendHotkey = (suspended: boolean) =>
  invoke<void>("suspend_hotkey", { suspended });
export const updateHotkey = (config: HotkeyConfig) =>
  invoke<void>("update_hotkey", { config });

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
}

export const listContextModes = () => invoke<ContextMode[]>("list_context_modes");
export const getContextMode = (id: string) =>
  invoke<ContextMode>("get_context_mode", { id });
export const createContextMode = (
  name: string,
  description: string,
  icon: string,
  color: string,
  writingStyle: string
) =>
  invoke<ContextMode>("create_context_mode", {
    name,
    description,
    icon,
    color,
    writingStyle,
  });
export const updateContextMode = (
  id: string,
  name: string,
  description: string,
  icon: string,
  color: string,
  writingStyle: string
) =>
  invoke<void>("update_context_mode", {
    id,
    name,
    description,
    icon,
    color,
    writingStyle,
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

// ── Structured Mode / LLM ────────────────────────────────────────────────
export interface LlmModelInfo {
  id: string;
  name: string;
  size_bytes: number;
  quantization: string;
  context_length: number;
  description: string;
  huggingface_repo: string;
  huggingface_file: string;
  is_downloaded: boolean;
  path: string | null;
  is_default: boolean;
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

export interface StructuredOutputPayload {
  markdown: string;
  slots: SlotExtraction;
  raw_transcript: string;
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
export const pasteStructuredOutput = (markdown: string) =>
  invoke<void>("paste_structured_output", { markdown });

export const onLlmDownloadProgress = (
  callback: (progress: LlmDownloadProgress) => void
): Promise<UnlistenFn> =>
  listen<LlmDownloadProgress>("llm-download-progress", (e) =>
    callback(e.payload)
  );

export const onLlmModelLoaded = (
  callback: (modelId: string) => void
): Promise<UnlistenFn> =>
  listen<string>("llm-model-loaded", (e) => callback(e.payload));

export const onStructuredOutputReady = (
  callback: (payload: StructuredOutputPayload) => void
): Promise<UnlistenFn> =>
  listen<StructuredOutputPayload>("structured-output-ready", (e) =>
    callback(e.payload)
  );

export const onStructuredModeDegraded = (
  callback: (reason: string) => void
): Promise<UnlistenFn> =>
  listen<string>("structured-mode-degraded", (e) => callback(e.payload));

// Event listeners
export const onRecordingStateChange = (
  callback: (status: string) => void
): Promise<UnlistenFn> => listen<string>("recording-state-change", (e) => callback(e.payload));

export const onAudioLevel = (
  callback: (level: number) => void
): Promise<UnlistenFn> => listen<number>("audio-level", (e) => callback(e.payload));

export const onDownloadProgress = (
  callback: (progress: DownloadProgress) => void
): Promise<UnlistenFn> =>
  listen<DownloadProgress>("download-progress", (e) => callback(e.payload));

export const onTranscriptionResult = (
  callback: (text: string) => void
): Promise<UnlistenFn> =>
  listen<string>("transcription-result", (e) => callback(e.payload));

export const onModelLoaded = (
  callback: (modelId: string) => void
): Promise<UnlistenFn> => listen<string>("model-loaded", (e) => callback(e.payload));

export interface RecordingError {
  state: string;
  code: string;
  message: string;
}

export const onRecordingError = (
  callback: (error: RecordingError) => void
): Promise<UnlistenFn> =>
  listen<RecordingError>("recording-error", (e) => callback(e.payload));

export const onContextModeChanged = (
  callback: (mode: { id: string; name: string; icon: string; color: string }) => void
): Promise<UnlistenFn> =>
  listen("context-mode-changed", (e) => callback(e.payload as { id: string; name: string; icon: string; color: string }));

export const onTranscriptionPreview = (
  callback: (text: string) => void
): Promise<UnlistenFn> =>
  listen<string>("transcription-preview", (e) => callback(e.payload));

export const onSettingsChanged = (
  callback: (settings: AppSettings) => void
): Promise<UnlistenFn> =>
  listen<AppSettings>("settings-changed", (e) => callback(e.payload));
