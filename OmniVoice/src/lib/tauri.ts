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

export interface AppSettings {
  theme: string;
  language: string;
  auto_start: boolean;
  minimize_to_tray: boolean;
  output_mode: string;
  sample_rate: number;
  active_model_id: string | null;
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
export const searchHistory = (query: string) =>
  invoke<TranscriptionRecord[]>("search_history", { query });
export const recentHistory = (limit?: number) =>
  invoke<TranscriptionRecord[]>("recent_history", { limit: limit ?? null });
export const deleteHistoryRecord = (id: string) =>
  invoke<void>("delete_history_record", { id });
export const exportHistory = (format: string) =>
  invoke<string>("export_history", { format });

// Settings commands
export const getSettings = () => invoke<AppSettings>("get_settings");
export const updateSettings = (settings: AppSettings) =>
  invoke<void>("update_settings", { settings });

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
