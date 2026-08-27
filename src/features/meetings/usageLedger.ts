import type { MeetingAiUsageRecord } from "@/lib/tauri";

export interface MeetingUsageRollup {
  key: string;
  meetingId: string | null;
  meetingTitle: string;
  cost: number;
  requests: number;
  promptTokens: number;
  completionTokens: number;
  latestAt: string;
}

export function rollupUsageByMeeting(records: MeetingAiUsageRecord[]): MeetingUsageRollup[] {
  const grouped = new Map<string, MeetingUsageRollup>();
  for (const record of records) {
    const key = record.meeting_id ?? `deleted:${record.id}`;
    const current = grouped.get(key) ?? {
      key,
      meetingId: record.meeting_id,
      meetingTitle: record.meeting_title?.trim() || "Deleted meeting",
      cost: 0,
      requests: 0,
      promptTokens: 0,
      completionTokens: 0,
      latestAt: record.created_at,
    };
    current.cost += Math.max(0, record.cost);
    current.requests += 1;
    current.promptTokens += Math.max(0, record.prompt_tokens);
    current.completionTokens += Math.max(0, record.completion_tokens);
    if (record.created_at > current.latestAt) current.latestAt = record.created_at;
    grouped.set(key, current);
  }
  return [...grouped.values()].sort((left, right) => right.cost - left.cost || right.latestAt.localeCompare(left.latestAt));
}

export function projectedMonthSpend(monthSpend: number, now = new Date()): number {
  if (!Number.isFinite(monthSpend) || monthSpend <= 0) return 0;
  const year = now.getFullYear();
  const month = now.getMonth();
  const daysInMonth = new Date(year, month + 1, 0).getDate();
  const elapsedDays = Math.max(1, now.getDate() - 1 + now.getHours() / 24 + now.getMinutes() / 1_440);
  return monthSpend * daysInMonth / elapsedDays;
}

export function nextLocalMonthLabel(now = new Date()): string {
  const reset = new Date(now.getFullYear(), now.getMonth() + 1, 1);
  return reset.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

export function usageRecordsCsv(records: MeetingAiUsageRecord[]): string {
  const escape = (value: unknown) => `"${String(value ?? "").replace(/"/g, '""')}"`;
  const header = ["time", "meeting", "request", "model", "cost_usd", "prompt_tokens", "completion_tokens"];
  const rows = records.map((record) => [
    record.created_at,
    record.meeting_title ?? "Deleted meeting",
    record.request_kind,
    record.model,
    record.cost.toFixed(8),
    record.prompt_tokens,
    record.completion_tokens,
  ]);
  return [header, ...rows].map((row) => row.map(escape).join(",")).join("\n");
}
