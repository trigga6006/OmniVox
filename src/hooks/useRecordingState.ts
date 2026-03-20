import { useCallback } from "react";
import { useRecordingStore } from "@/stores/recordingStore";
import { useTauriEvent } from "./useTauriEvent";
import { onRecordingStateChange, onAudioLevel, onTranscriptionResult } from "@/lib/tauri";
import type { RecordingStatus } from "@/stores/recordingStore";

export function useRecordingState() {
  const { setStatus, setAudioLevel, setLastTranscription } = useRecordingStore();

  const handleStateChange = useCallback(
    (status: string) => setStatus(status as RecordingStatus),
    [setStatus]
  );

  const handleAudioLevel = useCallback(
    (level: number) => setAudioLevel(level),
    [setAudioLevel]
  );

  const handleTranscription = useCallback(
    (text: string) => setLastTranscription(text),
    [setLastTranscription]
  );

  useTauriEvent(onRecordingStateChange, handleStateChange);
  useTauriEvent(onAudioLevel, handleAudioLevel);
  useTauriEvent(onTranscriptionResult, handleTranscription);
}
