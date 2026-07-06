import { describe, it, expect, beforeAll, afterAll, vi } from "vitest";
import type { AnalyticsRecord } from "@/lib/tauri";
import {
  computeAnalytics,
  compact,
  formatLongDuration,
  hourLabel,
  HEATMAP_WEEKS,
} from "./compute";

// Streaks, the heatmap grid, and "future" flags all depend on the current
// date — pin it so assertions are deterministic.
const NOW = new Date(2026, 6, 6, 12, 0, 0); // Mon Jul 6 2026, local noon

beforeAll(() => {
  vi.useFakeTimers();
  vi.setSystemTime(NOW);
});

afterAll(() => {
  vi.useRealTimers();
});

let nextId = 1;
function rec(
  createdAt: Date,
  words = 10,
  opts: Partial<Pick<AnalyticsRecord, "char_count" | "duration_ms" | "model_name">> = {}
): AnalyticsRecord {
  return {
    id: String(nextId++),
    created_at: createdAt.toISOString(),
    word_count: words,
    char_count: opts.char_count ?? words * 5,
    duration_ms: opts.duration_ms ?? 4000,
    model_name: opts.model_name ?? "whisper-a",
  } as AnalyticsRecord;
}

/** A Date `days` before NOW at the given local hour. */
function daysAgo(days: number, hour = 10): Date {
  const d = new Date(NOW);
  d.setDate(d.getDate() - days);
  d.setHours(hour, 0, 0, 0);
  return d;
}

describe("computeAnalytics", () => {
  it("returns zeroed data for no records", () => {
    const a = computeAnalytics([]);
    expect(a.totalMessages).toBe(0);
    expect(a.totalSessions).toBe(0);
    expect(a.totalWords).toBe(0);
    expect(a.currentStreak).toBe(0);
    expect(a.longestStreak).toBe(0);
    expect(a.avgWpm).toBe(0);
    expect(a.favoriteModel).toBeNull();
    expect(a.firstDate).toBeNull();
    expect(a.weeks).toHaveLength(HEATMAP_WEEKS);
  });

  it("splits sessions on gaps longer than 30 minutes", () => {
    const base = daysAgo(1, 9);
    const a = computeAnalytics([
      rec(new Date(base.getTime())),
      rec(new Date(base.getTime() + 10 * 60_000)), // +10min — same session
      rec(new Date(base.getTime() + 29 * 60_000)), // +19min after prev — same
      rec(new Date(base.getTime() + 65 * 60_000)), // +36min after prev — new session
    ]);
    expect(a.totalSessions).toBe(2);
  });

  it("counts a current streak ending today", () => {
    const a = computeAnalytics([rec(daysAgo(2)), rec(daysAgo(1)), rec(daysAgo(0))]);
    expect(a.currentStreak).toBe(3);
    expect(a.longestStreak).toBe(3);
  });

  it("keeps the current streak alive when today has no dictation yet", () => {
    const a = computeAnalytics([rec(daysAgo(2)), rec(daysAgo(1))]);
    expect(a.currentStreak).toBe(2);
  });

  it("breaks the current streak on a missed day but keeps the longest run", () => {
    const a = computeAnalytics([
      rec(daysAgo(6)),
      rec(daysAgo(5)),
      rec(daysAgo(4)),
      rec(daysAgo(3)), // 4-day run
      // daysAgo(2) and (1) missed
      rec(daysAgo(0)),
    ]);
    expect(a.currentStreak).toBe(1);
    expect(a.longestStreak).toBe(4);
  });

  it("computes speaking rate from recorded audio duration", () => {
    // 300 words over 2 minutes of audio → 150 WPM
    const a = computeAnalytics([rec(daysAgo(1), 300, { duration_ms: 120_000 })]);
    expect(a.avgWpm).toBe(150);
  });

  it("ranks models by usage with percentages", () => {
    const a = computeAnalytics([
      rec(daysAgo(1), 10, { model_name: "big" }),
      rec(daysAgo(1), 10, { model_name: "big" }),
      rec(daysAgo(1), 10, { model_name: "big" }),
      rec(daysAgo(1), 10, { model_name: "small" }),
    ]);
    expect(a.favoriteModel?.name).toBe("big");
    expect(a.favoriteModel?.count).toBe(3);
    expect(a.favoriteModel?.percent).toBe(75);
    expect(a.models.map((m) => m.name)).toEqual(["big", "small"]);
  });

  it("buckets dictations into local peak hours", () => {
    const a = computeAnalytics([
      rec(daysAgo(1, 9)),
      rec(daysAgo(2, 9)),
      rec(daysAgo(1, 22)),
    ]);
    expect(a.peakHour).toBe(9);
    expect(a.peakHours[9]).toBe(2);
    expect(a.peakHours[22]).toBe(1);
  });

  it("marks heatmap days after today as future", () => {
    const a = computeAnalytics([rec(daysAgo(0))]);
    const cells = a.weeks.flat();
    const todayKey = "2026-07-06";
    const todayCell = cells.find((c) => c.date === todayKey);
    expect(todayCell?.future).toBe(false);
    expect(todayCell?.words).toBeGreaterThan(0);
    // NOW is a Monday, so the trailing week has future days after it.
    const futureCells = cells.filter((c) => c.future);
    expect(futureCells.length).toBeGreaterThan(0);
    expect(futureCells.every((c) => c.words === 0)).toBe(true);
  });

  it("reports the trailing 30 days oldest→newest", () => {
    const a = computeAnalytics([rec(daysAgo(0), 42)]);
    expect(a.recentDaily).toHaveLength(30);
    expect(a.recentDaily[29].date).toBe("2026-07-06");
    expect(a.recentDaily[29].words).toBe(42);
    expect(a.recentDaily[0].words).toBe(0);
  });
});

describe("formatting helpers", () => {
  it("compacts large counts", () => {
    expect(compact(999)).toBe("999");
    expect(compact(1000)).toBe("1k");
    expect(compact(1200)).toBe("1.2k");
    expect(compact(12345)).toBe("12k");
    expect(compact(1_500_000)).toBe("1.5M");
  });

  it("formats long durations", () => {
    expect(formatLongDuration(45 * 60_000)).toBe("45m");
    expect(formatLongDuration(3 * 3_600_000 + 5 * 60_000)).toBe("3h 5m");
  });

  it("labels hours in 12-hour time", () => {
    expect(hourLabel(0)).toBe("12 AM");
    expect(hourLabel(12)).toBe("12 PM");
    expect(hourLabel(14)).toBe("2 PM");
  });
});
