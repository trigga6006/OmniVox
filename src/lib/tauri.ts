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
  live_preview: boolean;
  noise_reduction: boolean;
  auto_switch_modes: boolean;
}

export interface AppBinding {
  id: string;
  mode_id: string;
  process_name: string;
  created_at: string;
}

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
  llm_prompt: string;
  sort_order: number;
  is_builtin: boolean;
  created_at: string;
  updated_at: string;
}

export const listContextModes = () => invoke<ContextMode[]>("list_context_modes");
export const getContextMode = (id: string) =>
  invoke<ContextMode>("get_context_mode", { id });
export const createContextMode = (
  name: string,
  description: string,
  icon: string,
  color: string,
  llmPrompt: string
) =>
  invoke<ContextMode>("create_context_mode", {
    name,
    description,
    icon,
    color,
    llmPrompt,
  });
export const updateContextMode = (
  id: string,
  name: string,
  description: string,
  icon: string,
  color: string,
  llmPrompt: string
) =>
  invoke<void>("update_context_mode", {
    id,
    name,
    description,
    icon,
    color,
    llmPrompt,
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
