import { useCallback, useEffect, useRef } from "react";
import { useRecordingStore } from "@/stores/recordingStore";
import { useTauriEvent } from "./useTauriEvent";
import { onRecordingStateChange, onAudioLevel } from "@/lib/tauri";
import type { RecordingStatus } from "@/stores/recordingStore";

export function useRecordingState() {
  const { setStatus, setDuration, setAudioLevel } =
    useRecordingStore();

  // --- Tauri event handlers ---
  // Note: transcription-result is handled globally in App.tsx via
  // useGlobalTranscriptionSync() so hotkey dictations are captured
  // regardless of which page is active.

  const handleStateChange = useCallback(
    (status: string) => {
      setStatus(status as RecordingStatus);
      // Reset duration when entering recording or idle state
      if (status === "recording" || status === "idle") {
        setDuration(0);
      }
    },
    [setStatus, setDuration]
  );

  const handleAudioLevel = useCallback(
    (level: number) => setAudioLevel(level),
    [setAudioLevel]
  );

  useTauriEvent(onRecordingStateChange, handleStateChange);
  useTauriEvent(onAudioLevel, handleAudioLevel);

  // --- Duration timer: increments every 100ms while recording ---

  const status = useRecordingStore((s) => s.status);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    if (status === "recording") {
      intervalRef.current = setInterval(() => {
        setDuration(useRecordingStore.getState().duration + 100);
      }, 100);
    } else {
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
        intervalRef.current = null;
      }
    }

    return () => {
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
        intervalRef.current = null;
      }
    };
  }, [status, setDuration]);
}
