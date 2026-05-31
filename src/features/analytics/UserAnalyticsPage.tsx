import { useEffect, useMemo, useRef, useState } from "react";
import { Activity, BarChart3, CalendarDays, RefreshCw, Zap } from "lucide-react";
import { getAnalyticsRecords, type AnalyticsRecord } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import {
  computeAnalytics,
  compact,
  formatDayShort,
  formatLongDuration,
  hourLabel,
  prettyModel,
  type AnalyticsData,
  type HeatCell,
} from "./compute";

/** Instant hover tooltip anchored above a chart bar (parent must be `group relative`). */
function BarTooltip({ children }: { children: React.ReactNode }) {
  return (
    <div className="pointer-events-none absolute bottom-full left-1/2 z-20 mb-1.5 -translate-x-1/2 whitespace-nowrap rounded-md border border-border bg-surface-3 px-2 py-1 text-[10.5px] leading-none opacity-0 shadow-md transition-opacity duration-150 group-hover:opacity-100">
      {children}
    </div>
  );
}

export function UserAnalyticsPage() {
  const [records, setRecords] = useState<AnalyticsRecord[] | null>(null);
  const [loading, setLoading] = useState(true);
  const mountedRef = useRef(true);

  const load = () => {
    setLoading(true);
    getAnalyticsRecords()
      .then((data) => {
        if (mountedRef.current) setRecords(data);
      })
      .catch((e) => console.error("Failed to load analytics:", e))
      .finally(() => {
        if (mountedRef.current) setLoading(false);
      });
  };

  useEffect(() => {
    mountedRef.current = true;
    load();
    return () => {
      mountedRef.current = false;
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const data = useMemo(
    () => (records ? computeAnalytics(records) : null),
    [records]
  );

  return (
    <div className="flex h-full flex-col overflow-y-auto px-8 pt-6 pb-8">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="font-display text-2xl font-semibold tracking-[-0.02em] text-text-primary">
            Analytics
          </h1>
          <p className="mt-1 text-sm text-text-muted">
            Your dictation, by the numbers
          </p>
        </div>
        <button
          onClick={load}
          className="rounded-lg p-2 text-text-muted transition-colors hover:bg-surface-2/70 hover:text-text-secondary"
          title="Refresh"
        >
          <RefreshCw size={16} strokeWidth={1.75} />
        </button>
      </div>

      {loading && !data ? (
        <div className="flex flex-1 items-center justify-center">
          <div className="h-4 w-4 animate-spin rounded-full border-2 border-text-muted/25 border-t-amber-400" />
        </div>
      ) : !data || data.totalMessages === 0 ? (
        <EmptyState />
      ) : (
        <Dashboard data={data} />
      )}
    </div>
  );
}

function EmptyState() {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-4">
      <div className="relative flex items-center justify-center">
        <div className="absolute h-20 w-20 rounded-full border border-amber-500/15 bg-amber-500/[0.05]" />
        <BarChart3 size={36} strokeWidth={1.5} className="relative text-text-muted" />
      </div>
      <div className="mt-2 text-center">
        <p className="text-sm font-medium text-text-secondary">No data yet</p>
        <p className="mt-1 text-xs text-text-muted">
          Start dictating and your stats will appear here
        </p>
      </div>
    </div>
  );
}

function Dashboard({ data }: { data: AnalyticsData }) {
  return (
    <div className="mt-5 flex flex-col gap-4 pb-4">
      <OverviewCard data={data} />

      {/* Heatmap */}
      <HeatMapCard data={data} />

      {/* Peak hours + Models */}
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <PeakHoursCard data={data} />
        <ModelsCard data={data} />
      </div>

      {/* 30-day trend */}
      <TrendCard data={data} />
    </div>
  );
}

// ── Overview — single card, two ledger columns ───────────────────────────────

interface Row {
  label: string;
  value: string;
  accent?: boolean;
}

function OverviewCard({ data }: { data: AnalyticsData }) {
  const output: Row[] = [
    { label: "Words", value: data.totalWords.toLocaleString() },
    { label: "Characters", value: compact(data.totalChars) },
    { label: "Est. tokens", value: compact(data.estTokens) },
    { label: "Dictations", value: compact(data.totalMessages) },
    { label: "Sessions", value: compact(data.totalSessions) },
    { label: "Time recorded", value: formatLongDuration(data.totalDurationMs) },
  ];
  const consistency: Row[] = [
    {
      label: "Current streak",
      value: `${data.currentStreak} ${data.currentStreak === 1 ? "day" : "days"}`,
      accent: true,
    },
    {
      label: "Longest streak",
      value: `${data.longestStreak} ${data.longestStreak === 1 ? "day" : "days"}`,
    },
    { label: "Active days", value: compact(data.activeDays) },
    { label: "Avg pace", value: `${Math.round(data.avgWpm)} wpm` },
    { label: "Per active day", value: `${compact(Math.round(data.avgWordsPerDay))} words` },
    { label: "Favorite model", value: prettyModel(data.favoriteModel?.name ?? "—") },
  ];

  return (
    <section
      className="animate-slide-up rounded-xl border border-border bg-surface-1/85 p-6"
      style={{ opacity: 0, animationDelay: "0.05s", animationFillMode: "forwards" }}
    >
      <div className="grid grid-cols-1 gap-6 sm:grid-cols-2 sm:gap-0">
        <LedgerColumn title="Output" rows={output} className="sm:pr-8" />
        <LedgerColumn
          title="Consistency"
          rows={consistency}
          className="sm:border-l sm:border-border/60 sm:pl-8"
        />
      </div>
    </section>
  );
}

function LedgerColumn({
  title,
  rows,
  className,
}: {
  title: string;
  rows: Row[];
  className?: string;
}) {
  return (
    <div className={className}>
      <div className="mb-1 text-[10.5px] font-semibold uppercase tracking-[0.12em] text-text-muted">
        {title}
      </div>
      <div className="flex flex-col">
        {rows.map((row, i) => (
          <div
            key={row.label}
            className={cn(
              "flex items-center justify-between gap-3 py-2.5 text-[13px]",
              i > 0 && "border-t border-border/40"
            )}
          >
            <span className="shrink-0 text-text-muted">{row.label}</span>
            <span
              className={cn(
                "truncate font-mono font-medium tabular-nums",
                row.accent ? "text-amber-300 light:text-amber-700" : "text-text-primary"
              )}
              title={row.value}
            >
              {row.value}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

// ── Section card shell ───────────────────────────────────────────────────────

function SectionCard({
  icon: Icon,
  title,
  aside,
  children,
  delay,
}: {
  icon: typeof Activity;
  title: string;
  aside?: React.ReactNode;
  children: React.ReactNode;
  delay: number;
}) {
  return (
    <section
      className="animate-slide-up rounded-xl border border-border bg-surface-1/85 p-5"
      style={{ opacity: 0, animationDelay: `${delay}s`, animationFillMode: "forwards" }}
    >
      <div className="mb-4 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Icon size={14} strokeWidth={2} className="text-text-muted" />
          <span className="text-[10.5px] font-semibold uppercase tracking-[0.12em] text-text-muted">
            {title}
          </span>
        </div>
        {aside}
      </div>
      {children}
    </section>
  );
}

// ── Heatmap ──────────────────────────────────────────────────────────────────

const HEAT_COLORS: Record<number, string> = {
  0: "var(--heat-0)",
  1: "var(--heat-1)",
  2: "var(--heat-2)",
  3: "var(--heat-3)",
  4: "var(--heat-4)",
};

function cellStyle(cell: HeatCell): React.CSSProperties {
  if (cell.future) return { background: "transparent" };
  const style: React.CSSProperties = { background: HEAT_COLORS[cell.level] };
  if (cell.level === 4) style.boxShadow = "0 0 6px -1px var(--color-amber-500)";
  return style;
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const WEEKDAY_LABELS = ["", "Mon", "", "Wed", "", "Fri", ""];

function HeatMapCard({ data }: { data: AnalyticsData }) {
  // Month labels above the columns where a new month begins.
  const monthMarks = data.weeks.map((week, w) => {
    const top = week[0];
    const [, m, d] = top.date.split("-").map(Number);
    const prev = w > 0 ? data.weeks[w - 1][0].date.split("-").map(Number) : null;
    const isNewMonth = w === 0 ? d <= 7 : prev && m !== prev[1];
    return isNewMonth ? MONTHS[m - 1] : "";
  });

  return (
    <SectionCard
      icon={CalendarDays}
      title="Activity"
      delay={0.42}
      aside={
        <div className="flex items-center gap-1.5 text-[10px] text-text-muted">
          <span>Less</span>
          {[0, 1, 2, 3, 4].map((l) => (
            <span
              key={l}
              className="h-[10px] w-[10px] rounded-[2px]"
              style={cellStyle({ level: l, future: false } as HeatCell)}
            />
          ))}
          <span>More</span>
        </div>
      }
    >
      <div className="flex flex-col gap-1">
        {/* Month labels — same flex structure as the grid below so they align. */}
        <div className="flex gap-[3px]">
          <div className="w-5 shrink-0" />
          <div className="flex flex-1 gap-[3px]">
            {monthMarks.map((label, i) => (
              <div key={i} className="max-w-[16px] flex-1">
                <span className="block whitespace-nowrap text-[9px] text-text-muted">
                  {label}
                </span>
              </div>
            ))}
          </div>
        </div>
        {/* Grid */}
        <div className="flex items-stretch gap-[3px]">
          {/* Weekday labels */}
          <div className="flex w-5 shrink-0 flex-col gap-[3px]">
            {WEEKDAY_LABELS.map((label, i) => (
              <span
                key={i}
                className="flex-1 text-right text-[9px] leading-none text-text-muted"
              >
                {label}
              </span>
            ))}
          </div>
          {data.weeks.map((week, w) => (
            <div key={w} className="flex max-w-[16px] flex-1 flex-col gap-[3px]">
              {week.map((cell) => (
                <div
                  key={cell.date}
                  className="aspect-square w-full rounded-[2px]"
                  style={cellStyle(cell)}
                  title={
                    cell.future
                      ? undefined
                      : `${cell.words.toLocaleString()} ${cell.words === 1 ? "word" : "words"} · ${formatDayShort(cell.date)}`
                  }
                />
              ))}
            </div>
          ))}
        </div>
      </div>
    </SectionCard>
  );
}

// ── Peak hours ───────────────────────────────────────────────────────────────

function PeakHoursCard({ data }: { data: AnalyticsData }) {
  const max = Math.max(1, ...data.peakHours);
  const hasData = data.peakHours.some((v) => v > 0);

  return (
    <SectionCard icon={BarChart3} title="Peak hours" delay={0.48}>
      <div className="flex h-[88px] items-end gap-[3px]">
        {data.peakHours.map((count, h) => {
          const isPeak = h === data.peakHour && count > 0;
          return (
            <div
              key={h}
              className="group relative flex flex-1 items-end"
              style={{ height: "100%" }}
            >
              <div
                className="w-full rounded-[2px] transition-colors"
                style={{
                  height: `${Math.max(count > 0 ? 6 : 2, (count / max) * 100)}%`,
                  background: isPeak
                    ? "var(--bar-fill-strong)"
                    : count > 0
                      ? "var(--bar-fill)"
                      : "var(--bar-empty)",
                  boxShadow: isPeak ? "0 0 7px -1px var(--color-amber-500)" : undefined,
                }}
              />
              <BarTooltip>
                <span className="font-mono tabular-nums text-text-primary">{count}</span>{" "}
                <span className="text-text-muted">
                  {count === 1 ? "dictation" : "dictations"} · {hourLabel(h)}
                </span>
              </BarTooltip>
            </div>
          );
        })}
      </div>
      <div className="mt-2 flex justify-between text-[9px] text-text-muted">
        <span>12a</span>
        <span>6a</span>
        <span>12p</span>
        <span>6p</span>
        <span>12a</span>
      </div>
      <p className="mt-3 text-xs text-text-secondary">
        {hasData ? (
          <>
            Most active around{" "}
            <span className="font-medium text-amber-300 light:text-amber-700">
              {hourLabel(data.peakHour)}
            </span>
          </>
        ) : (
          "Not enough data yet"
        )}
      </p>
    </SectionCard>
  );
}

// ── Models breakdown ─────────────────────────────────────────────────────────

function ModelsCard({ data }: { data: AnalyticsData }) {
  const top = data.models.slice(0, 5);
  return (
    <SectionCard icon={Zap} title="Models used" delay={0.54}>
      <div className="flex flex-col gap-3">
        {top.map((m) => (
          <div key={m.name}>
            <div className="mb-1 flex items-baseline justify-between">
              <span className="text-[13px] text-text-secondary">{prettyModel(m.name)}</span>
              <span className="font-mono text-[11px] tabular-nums text-text-muted">
                {compact(m.count)} · {Math.round(m.percent)}%
              </span>
            </div>
            <div className="h-[6px] overflow-hidden rounded-full bg-surface-2">
              <div
                className="h-full rounded-full"
                style={{
                  width: `${Math.max(2, m.percent)}%`,
                  background: "var(--bar-fill-strong)",
                }}
              />
            </div>
          </div>
        ))}
      </div>
    </SectionCard>
  );
}

// ── 30-day trend ─────────────────────────────────────────────────────────────

function TrendCard({ data }: { data: AnalyticsData }) {
  const max = Math.max(1, ...data.recentDaily.map((d) => d.words));
  const total = data.recentDaily.reduce((s, d) => s + d.words, 0);
  const todayKey = data.recentDaily[data.recentDaily.length - 1]?.date;

  return (
    <SectionCard
      icon={Activity}
      title="Last 30 days"
      delay={0.6}
      aside={
        <span className="font-mono text-[11px] tabular-nums text-text-muted">
          {compact(total)} words
        </span>
      }
    >
      <div className="flex h-[72px] items-end gap-[3px]">
        {data.recentDaily.map((d) => {
          const isToday = d.date === todayKey;
          return (
            <div
              key={d.date}
              className="group relative flex flex-1 items-end"
              style={{ height: "100%" }}
            >
              <div
                className="w-full rounded-[2px]"
                style={{
                  height: `${Math.max(d.words > 0 ? 4 : 2, (d.words / max) * 100)}%`,
                  background:
                    d.words === 0
                      ? "var(--bar-empty)"
                      : isToday
                        ? "var(--bar-fill-strong)"
                        : "var(--bar-fill)",
                }}
              />
              <BarTooltip>
                <span className="font-mono tabular-nums text-text-primary">
                  {d.words.toLocaleString()}
                </span>{" "}
                <span className="text-text-muted">
                  {d.words === 1 ? "word" : "words"} · {formatDayShort(d.date)}
                  {isToday ? " (today)" : ""}
                </span>
              </BarTooltip>
            </div>
          );
        })}
      </div>
    </SectionCard>
  );
}
