import { useCallback, useEffect, useState } from "react";
import {
  getMeetingState,
  onMeetingState,
  type MeetingRuntimeState,
} from "@/lib/tauri";

const IDLE: MeetingRuntimeState = {
  meeting_id: null,
  status: "idle",
  elapsed_ms: 0,
  mic_level: 0,
  system_level: 0,
  mic_signal_detected: false,
  system_signal_detected: false,
  capture_warning: null,
};

export function useMeetingRuntime() {
  const [runtime, setRuntime] = useState<MeetingRuntimeState>(IDLE);

  const refresh = useCallback(() => {
    getMeetingState().then(setRuntime).catch(() => setRuntime(IDLE));
  }, []);

  useEffect(() => {
    refresh();
    const unlisten = onMeetingState(setRuntime);
    const timer = window.setInterval(() => {
      getMeetingState().then(setRuntime).catch(() => {});
    }, 500);
    return () => {
      window.clearInterval(timer);
      unlisten.then((fn) => fn());
    };
  }, [refresh]);

  return { runtime, refresh };
}

export function formatMeetingDuration(milliseconds: number) {
  const totalSeconds = Math.floor(milliseconds / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  return hours > 0
    ? `${hours}:${minutes.toString().padStart(2, "0")}:${seconds.toString().padStart(2, "0")}`
    : `${minutes}:${seconds.toString().padStart(2, "0")}`;
}
