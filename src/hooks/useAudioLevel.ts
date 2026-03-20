import { useCallback } from "react";
import { useRecordingStore } from "@/stores/recordingStore";
import { useTauriEvent } from "./useTauriEvent";
import { onAudioLevel } from "@/lib/tauri";

export function useAudioLevel() {
  const setAudioLevel = useRecordingStore((s) => s.setAudioLevel);

  const handleLevel = useCallback(
    (level: number) => setAudioLevel(level),
    [setAudioLevel]
  );

  useTauriEvent(onAudioLevel, handleLevel);

  return useRecordingStore((s) => s.audioLevel);
}
