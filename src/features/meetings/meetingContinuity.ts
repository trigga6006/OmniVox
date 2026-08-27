import type { Meeting } from "@/lib/tauri";

export function normalizeMeetingTitle(title: string): string {
  return title
    .normalize("NFKC")
    .toLocaleLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim()
    .replace(/\s+/g, " ");
}

export function findPreviousMeeting(meetings: Meeting[], title: string): Meeting | null {
  const normalized = normalizeMeetingTitle(title);
  if (!normalized) return null;
  return meetings
    .filter((meeting) => !meeting.deleted_at && normalizeMeetingTitle(meeting.title) === normalized)
    .sort((left, right) => right.started_at.localeCompare(left.started_at))[0] ?? null;
}
