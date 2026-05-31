import type { AnalyticsRecord } from "@/lib/tauri";

/** A gap longer than this between consecutive dictations starts a new session. */
const SESSION_GAP_MS = 30 * 60 * 1000;

/** Number of week-columns rendered in the contribution heatmap. */
export const HEATMAP_WEEKS = 53;

export interface ModelUsage {
  name: string;
  count: number;
  percent: number;
}

export interface HeatCell {
  /** Local YYYY-MM-DD. */
  date: string;
  words: number;
  count: number;
  /** 0 (none) … 4 (most). */
  level: number;
  /** A future day inside the trailing week — rendered as an empty placeholder. */
  future: boolean;
}

export interface DailyPoint {
  date: string;
  words: number;
}

export interface AnalyticsData {
  totalMessages: number;
  totalSessions: number;
  totalWords: number;
  totalChars: number;
  estTokens: number;
  totalDurationMs: number;
  activeDays: number;
  currentStreak: number;
  longestStreak: number;
  /** Speaking rate: words per minute of recorded audio. */
  avgWpm: number;
  avgWordsPerDay: number;
  avgWordsPerSession: number;
  favoriteModel: ModelUsage | null;
  models: ModelUsage[];
  /** 24 buckets, counts of dictations started in each local hour. */
  peakHours: number[];
  peakHour: number;
  /** Week-major grid: weeks[w][dayOfWeek 0=Sun]. */
  weeks: HeatCell[][];
  heatmapMaxWords: number;
  /** Words per day for the trailing 30 days (oldest → newest). */
  recentDaily: DailyPoint[];
  firstDate: string | null;
}

/** Local YYYY-MM-DD key for a Date. */
function dayKey(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/** Integer day-index in local time (days since epoch), for streak runs. */
function dayNumber(d: Date): number {
  const local = d.getTime() - d.getTimezoneOffset() * 60_000;
  return Math.floor(local / 86_400_000);
}

function levelFor(words: number, max: number): number {
  if (words <= 0 || max <= 0) return 0;
  const r = words / max;
  if (r >= 0.75) return 4;
  if (r >= 0.5) return 3;
  if (r >= 0.25) return 2;
  return 1;
}

export function computeAnalytics(records: AnalyticsRecord[]): AnalyticsData {
  const sorted = [...records].sort(
    (a, b) => new Date(a.created_at).getTime() - new Date(b.created_at).getTime()
  );

  const totalMessages = sorted.length;
  let totalWords = 0;
  let totalChars = 0;
  let totalDurationMs = 0;
  let totalSessions = 0;
  let prevMs = -Infinity;

  const peakHours = new Array(24).fill(0);
  const modelCounts = new Map<string, number>();
  const wordsByDay = new Map<string, number>();
  const activeSet = new Set<string>();

  for (const rec of sorted) {
    const d = new Date(rec.created_at);
    const ms = d.getTime();

    totalWords += rec.word_count;
    totalChars += rec.char_count;
    totalDurationMs += rec.duration_ms;

    if (ms - prevMs > SESSION_GAP_MS) totalSessions += 1;
    prevMs = ms;

    peakHours[d.getHours()] += 1;
    modelCounts.set(rec.model_name, (modelCounts.get(rec.model_name) ?? 0) + 1);

    const key = dayKey(d);
    activeSet.add(key);
    wordsByDay.set(key, (wordsByDay.get(key) ?? 0) + rec.word_count);
  }

  const activeDays = activeSet.size;

  // Streaks
  const dayNums = [...activeSet]
    .map((k) => {
      const [y, m, d] = k.split("-").map(Number);
      return dayNumber(new Date(y, m - 1, d));
    })
    .sort((a, b) => a - b);

  let longestStreak = dayNums.length ? 1 : 0;
  let run = longestStreak;
  for (let i = 1; i < dayNums.length; i++) {
    if (dayNums[i] === dayNums[i - 1] + 1) run += 1;
    else if (dayNums[i] !== dayNums[i - 1]) run = 1;
    if (run > longestStreak) longestStreak = run;
  }

  let currentStreak = 0;
  const cursor = new Date();
  cursor.setHours(0, 0, 0, 0);
  if (!activeSet.has(dayKey(cursor))) cursor.setDate(cursor.getDate() - 1);
  while (activeSet.has(dayKey(cursor))) {
    currentStreak += 1;
    cursor.setDate(cursor.getDate() - 1);
  }

  // Models
  const models: ModelUsage[] = [...modelCounts.entries()]
    .map(([name, count]) => ({
      name,
      count,
      percent: totalMessages ? (count / totalMessages) * 100 : 0,
    }))
    .sort((a, b) => b.count - a.count);

  const peakHour = peakHours.reduce(
    (best, v, i) => (v > peakHours[best] ? i : best),
    0
  );

  // Heatmap grid — trailing HEATMAP_WEEKS columns, aligned Sun→Sat.
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const startSunday = new Date(today);
  startSunday.setDate(today.getDate() - today.getDay() - (HEATMAP_WEEKS - 1) * 7);

  const heatmapMaxWords = Math.max(0, ...wordsByDay.values());
  const weeks: HeatCell[][] = [];
  for (let w = 0; w < HEATMAP_WEEKS; w++) {
    const col: HeatCell[] = [];
    for (let dow = 0; dow < 7; dow++) {
      const d = new Date(startSunday);
      d.setDate(startSunday.getDate() + w * 7 + dow);
      const key = dayKey(d);
      const words = wordsByDay.get(key) ?? 0;
      col.push({
        date: key,
        words,
        count: activeSet.has(key) ? 1 : 0,
        level: levelFor(words, heatmapMaxWords),
        future: d.getTime() > today.getTime(),
      });
    }
    weeks.push(col);
  }

  // Trailing 30 days
  const recentDaily: DailyPoint[] = [];
  for (let i = 29; i >= 0; i--) {
    const d = new Date(today);
    d.setDate(today.getDate() - i);
    const key = dayKey(d);
    recentDaily.push({ date: key, words: wordsByDay.get(key) ?? 0 });
  }

  return {
    totalMessages,
    totalSessions,
    totalWords,
    totalChars,
    estTokens: Math.round(totalChars / 4),
    totalDurationMs,
    activeDays,
    currentStreak,
    longestStreak,
    avgWpm: totalDurationMs > 0 ? totalWords / (totalDurationMs / 60_000) : 0,
    avgWordsPerDay: activeDays ? totalWords / activeDays : 0,
    avgWordsPerSession: totalSessions ? totalWords / totalSessions : 0,
    favoriteModel: models[0] ?? null,
    models,
    peakHours,
    peakHour,
    weeks,
    heatmapMaxWords,
    recentDaily,
    firstDate: sorted.length ? sorted[0].created_at : null,
  };
}

// ── Formatting helpers ──────────────────────────────────────────────────────

/** Compact large counts: 12345 → "12.3k". */
export function compact(n: number): string {
  if (n < 1000) return Math.round(n).toLocaleString();
  if (n < 1_000_000) {
    const v = n / 1000;
    return `${v.toFixed(v < 10 ? 1 : 0).replace(/\.0$/, "")}k`;
  }
  const v = n / 1_000_000;
  return `${v.toFixed(1).replace(/\.0$/, "")}M`;
}

/** Total recorded time → "12h 34m" / "34m". */
export function formatLongDuration(ms: number): string {
  const totalMin = Math.round(ms / 60_000);
  const h = Math.floor(totalMin / 60);
  const m = totalMin % 60;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

/** Local day key "2026-05-03" → "May 3" (parsed as local, no TZ shift). */
export function formatDayShort(key: string): string {
  const [y, m, d] = key.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  });
}

/** 14 → "2 PM". */
export function hourLabel(h: number): string {
  const period = h < 12 ? "AM" : "PM";
  const hr = h % 12 === 0 ? 12 : h % 12;
  return `${hr} ${period}`;
}

/**
 * Clean model name for display. `model_name` is stored as the full model file
 * path (e.g. `…\ggml-distil-large-v3.bin`), so reduce it to the basename,
 * strip the `ggml-` prefix and `.bin` suffix, then prettify:
 * `…\ggml-distil-large-v3.bin` → "Distil Large V3", `…\ggml-base.en.bin` → "Base EN".
 */
export function prettyModel(name: string): string {
  if (!name) return "—";
  let base = name.split(/[\\/]/).pop() ?? name;
  base = base.replace(/\.bin$/i, "").replace(/^ggml[-_]/i, "");
  return base
    .split(/[-_.\s]+/)
    .filter(Boolean)
    .map((part) =>
      part.length <= 2 ? part.toUpperCase() : part.charAt(0).toUpperCase() + part.slice(1)
    )
    .join(" ");
}
