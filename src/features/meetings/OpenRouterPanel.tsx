import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import {
  Check,
  ChevronDown,
  CircleDollarSign,
  Copy,
  ExternalLink,
  KeyRound,
  Loader2,
  RefreshCw,
  ReceiptText,
  Search,
  ShieldCheck,
  Sparkles,
  Trash2,
} from "lucide-react";
import { Button, Checkbox, Progress, Select, Textarea, Toggle } from "@/components/ui";
import {
  getMeetingProviderSettings,
  getMeetingUsageStats,
  getOpenRouterStatus,
  listMeetingAiUsage,
  listOpenRouterModels,
  openOpenRouterCredits,
  openOpenRouterKeys,
  removeOpenRouterKey,
  saveMeetingProviderSettings,
  saveOpenRouterKey,
  type MeetingProviderSettings,
  type MeetingAiUsageRecord,
  type MeetingUsageStats,
  type OpenRouterModel,
  type ProviderStatus,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";
import {
  MEETING_LENGTH_OPTIONS,
  assessModelForMeeting,
  compactTokens,
  estimatedMeetingsRemaining,
  sortModelsForCatalog,
  type ModelCatalogSort,
} from "./modelCatalog";
import {
  nextLocalMonthLabel,
  projectedMonthSpend,
  rollupUsageByMeeting,
  usageRecordsCsv,
} from "./usageLedger";

const DEFAULTS: MeetingProviderSettings = {
  provider: "openrouter",
  model: "deepseek/deepseek-v4-flash",
  monthly_budget: 2,
  per_meeting_budget: 0.05,
  max_prompt_price: 0.5,
  max_completion_price: 3,
  zdr_only: true,
  deny_data_collection: true,
  auto_suggest: true,
  auto_summarize: false,
  summary_preset: "general",
  custom_instructions: "",
  transcription_mode: "after_meeting",
  routing_preference: "price",
};

const SUMMARY_PRESETS = [
  ["general", "General notes"],
  ["executive", "Executive brief"],
  ["one_on_one", "1:1 conversation"],
  ["sales", "Sales call"],
  ["interview", "Interview"],
  ["standup", "Standup"],
] as const;

const SORT_OPTIONS: Array<{ value: ModelCatalogSort; label: string }> = [
  { value: "cost", label: "Estimated cost" },
  { value: "value", label: "Quality per dollar" },
  { value: "quality", label: "Broad quality benchmark" },
  { value: "context", label: "Context size" },
  { value: "name", label: "Name" },
];

const TRANSCRIPTION_OPTIONS: Array<{ value: MeetingProviderSettings["transcription_mode"]; label: string }> = [
  { value: "after_meeting", label: "After meeting — lowest call overhead" },
  { value: "live", label: "During meeting — live transcript" },
];

const ROUTING_OPTIONS: Array<{ value: MeetingProviderSettings["routing_preference"]; label: string }> = [
  { value: "price", label: "Lowest cost" },
  { value: "balanced", label: "Balanced reliability · OpenRouter default" },
  { value: "throughput", label: "Fastest output" },
  { value: "latency", label: "Fastest response start" },
];

const LEDGER_KIND_OPTIONS = [
  { value: "all", label: "All requests" },
  { value: "summary", label: "Summaries" },
  { value: "question", label: "Questions" },
  { value: "legacy", label: "Legacy" },
];

/** Dense popover control height — one step below the default 32px row. */
const DENSE_CONTROL = "h-[var(--control-h-s)] text-xs";

function money(value: number | null | undefined) {
  return value == null ? "—" : `$${value.toFixed(value < 0.01 ? 4 : 2)}`;
}

export function OpenRouterPanel({ compact = false }: { compact?: boolean }) {
  const [settings, setSettings] = useState<MeetingProviderSettings>(DEFAULTS);
  const [status, setStatus] = useState<ProviderStatus | null>(null);
  const [usage, setUsage] = useState<MeetingUsageStats | null>(null);
  const [models, setModels] = useState<OpenRouterModel[]>([]);
  const [usageRecords, setUsageRecords] = useState<MeetingAiUsageRecord[]>([]);
  const [usageOpen, setUsageOpen] = useState(false);
  const [usageLoading, setUsageLoading] = useState(false);
  const [apiKey, setApiKey] = useState("");
  const [query, setQuery] = useState("");
  const [modelOpen, setModelOpen] = useState(false);
  const [meetingMinutes, setMeetingMinutes] = useState(60);
  const [modelSort, setModelSort] = useState<ModelCatalogSort>("cost");
  const [onlyWithinLimits, setOnlyWithinLimits] = useState(false);
  const [advanced, setAdvanced] = useState(false);
  const [busy, setBusy] = useState(false);
  const [loadingModels, setLoadingModels] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const refreshStatus = useCallback(() => {
    getOpenRouterStatus().then(setStatus).catch((error) =>
      setStatus({ ...({} as ProviderStatus), key_present: false, connected: false, error: String(error) })
    );
    getMeetingUsageStats().then(setUsage).catch(() => {});
  }, []);

  useEffect(() => {
    void getMeetingProviderSettings().then(setSettings).catch(() => {});
    refreshStatus();
  }, [refreshStatus]);

  const loadModels = useCallback(() => {
    if (models.length > 0 || loadingModels) return;
    setLoadingModels(true);
    listOpenRouterModels()
      .then(setModels)
      .catch((error) => setMessage(String(error)))
      .finally(() => setLoadingModels(false));
  }, [loadingModels, models.length]);

  const toggleUsage = useCallback(() => {
    setUsageOpen((open) => {
      const next = !open;
      if (next) {
        setUsageLoading(true);
        listMeetingAiUsage()
          .then(setUsageRecords)
          .catch((error) => setMessage(String(error)))
          .finally(() => setUsageLoading(false));
      }
      return next;
    });
  }, []);

  useEffect(() => {
    if (status?.connected) loadModels();
  }, [loadModels, status?.connected]);

  const selected = models.find((model) => model.id === settings.model);
  const selectedAssessment = selected
    ? assessModelForMeeting(selected, settings, usage, meetingMinutes)
    : null;
  const meetingsRemaining = selected
    ? estimatedMeetingsRemaining(selected, settings, usage, meetingMinutes)
    : null;
  const filtered = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    let source = normalized
      ? models.filter((model) => `${model.name} ${model.id}`.toLowerCase().includes(normalized))
      : models;
    if (onlyWithinLimits) {
      source = source.filter((model) => assessModelForMeeting(model, settings, usage, meetingMinutes).fits);
    }
    return sortModelsForCatalog(source, modelSort, settings, usage, meetingMinutes).slice(0, 100);
  }, [meetingMinutes, modelSort, models, onlyWithinLimits, query, settings, usage]);

  const persist = useCallback(async (next: MeetingProviderSettings) => {
    setSettings(next);
    try {
      await saveMeetingProviderSettings(next);
      setMessage("Saved");
      window.setTimeout(() => setMessage(null), 1200);
    } catch (error) {
      setMessage(String(error));
    }
  }, []);

  const connect = useCallback(async () => {
    if (!apiKey.trim()) return;
    setBusy(true);
    setMessage(null);
    try {
      const next = await saveOpenRouterKey(apiKey);
      setStatus(next);
      setApiKey("");
      setMessage("OpenRouter connected");
      loadModels();
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }, [apiKey, loadModels]);

  const disconnect = useCallback(async () => {
    setBusy(true);
    try {
      await removeOpenRouterKey();
      setStatus(null);
      refreshStatus();
      setMessage("OpenRouter disconnected");
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }, [refreshStatus]);

  const monthPercent = settings.monthly_budget > 0
    ? Math.min(100, ((usage?.month_spend ?? 0) / settings.monthly_budget) * 100)
    : 0;

  return (
    <section className={cn("rounded-2xl border border-border bg-surface-1", compact ? "p-4" : "p-5")}>
      <div className="flex items-start gap-3">
        <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-[var(--radius-l)] bg-amber-500/10 text-amber-300">
          <CircleDollarSign size={18} />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <h3 className="text-sm font-semibold text-text-primary">OpenRouter</h3>
            <span className={cn("h-1.5 w-1.5 rounded-full", status?.connected ? "bg-success" : "bg-text-muted/40")} />
            <span className="text-xs text-text-muted">{status?.connected ? "Connected" : "Not connected"}</span>
          </div>
          <p className="mt-1 text-xs leading-relaxed text-text-muted">
            Meeting text and context are sent only for AI actions. Audio stays on this device.
          </p>
        </div>
        {status?.connected && (
          <button
            aria-label="Refresh OpenRouter status"
            onClick={refreshStatus}
            className="rounded-[var(--radius-m)] p-2 text-text-muted transition-colors duration-[var(--dur-2)] ease-out hover:bg-surface-2 hover:text-text-primary"
          >
            <RefreshCw size={14} />
          </button>
        )}
      </div>

      {!status?.connected ? (
        <div className="mt-4 rounded-[var(--radius-l)] border border-border bg-surface-0/60 p-3">
          <label htmlFor="openrouter-key" className="eyebrow mb-2 flex items-center gap-1.5">
            <KeyRound size={12} /> API key
          </label>
          <div className="flex gap-2">
            <input
              id="openrouter-key"
              type="password"
              autoComplete="off"
              spellCheck={false}
              value={apiKey}
              onChange={(event) => setApiKey(event.target.value)}
              onKeyDown={(event) => { if (event.key === "Enter") connect(); }}
              placeholder="sk-or-v1-…"
              className="h-9 min-w-0 flex-1 rounded-[var(--radius-m)] border border-border-hover bg-surface-2 px-3 font-mono text-xs text-text-primary outline-none focus:border-amber-500/60"
            />
            <Button size="sm" onClick={connect} disabled={busy || !apiKey.trim()}>
              {busy ? <Loader2 className="animate-spin" /> : "Save & test"}
            </Button>
          </div>
          <p className="mt-2 text-xs text-text-muted">Entered here and stored privately by Windows for OmniVox.</p>
        </div>
      ) : (
        <>
          <div className="mt-4 grid grid-cols-3 gap-2">
            <Metric label="Key remaining" value={money(status.limit_remaining)} />
            <Metric label="This month" value={money(status.usage_monthly)} />
            <Metric
              label={status.limit_reset ? `Key limit · ${status.limit_reset}` : "Key limit"}
              value={status.limit == null ? "No limit" : money(status.limit)}
            />
          </div>
          {status.expires_at && (
            <p className="mt-2 text-xs text-text-muted">
              Key expires {new Date(status.expires_at).toLocaleDateString()}
            </p>
          )}

          <div className="mt-3 rounded-[var(--radius-l)] border border-border bg-surface-0/45 p-3">
            <div className="flex items-center justify-between text-xs text-text-muted">
              <span>OmniVox monthly guardrail</span>
              <span className="tnum font-mono">
                {money(usage?.month_spend ?? 0)} / {money(settings.monthly_budget)}
              </span>
            </div>
            <Progress
              className="mt-2 h-1.5"
              value={monthPercent}
              aria-label="Monthly AI spend against your budget"
            />
            <div className="mt-2 flex justify-between text-xs text-text-muted">
              <span className="tnum">{usage?.request_count ?? 0} AI requests</span>
              <span className="tnum">
                {((usage?.prompt_tokens ?? 0) + (usage?.completion_tokens ?? 0)).toLocaleString()} tokens
              </span>
            </div>
            <div className="mt-2 flex items-center justify-between border-t border-border pt-2 text-xs">
              <span className="text-text-muted">
                Projected month: {money(projectedMonthSpend(usage?.month_spend ?? 0))} · resets {nextLocalMonthLabel()}
              </span>
              <button
                onClick={toggleUsage}
                className="flex items-center gap-1 rounded-[var(--radius-s)] px-1.5 py-1 font-medium text-text-secondary transition-colors duration-[var(--dur-2)] ease-out hover:bg-surface-2 hover:text-text-primary"
              >
                <ReceiptText size={10} />
                {usageOpen ? "Hide activity" : "View activity"}
              </button>
            </div>
          </div>

          {usageOpen && (
            <UsageLedger
              records={usageRecords}
              loading={usageLoading}
              onCopy={() => {
                navigator.clipboard
                  .writeText(usageRecordsCsv(usageRecords))
                  .then(() => {
                    setMessage("Usage CSV copied");
                    window.setTimeout(() => setMessage(null), 1400);
                  })
                  .catch((error) => setMessage(String(error)));
              }}
            />
          )}

          <div className="relative mt-4">
            <span className="eyebrow mb-2 block">Default model</span>
            <button
              onClick={() => { setModelOpen((open) => !open); loadModels(); }}
              className="flex w-full items-center gap-3 rounded-[var(--radius-l)] border border-border-hover bg-surface-2 px-3 py-2.5 text-left transition-colors duration-[var(--dur-2)] ease-out hover:bg-surface-3"
            >
              <div className="min-w-0 flex-1">
                <div className="truncate text-sm font-medium text-text-primary">
                  {selected?.name ?? settings.model}
                </div>
                <div className="mt-0.5 truncate text-xs text-text-muted">
                  {selectedAssessment
                    ? `${money(selectedAssessment.estimatedCost)} planning estimate for ${meetingMinutes} min · ${compactTokens(selected!.context_length)} context${
                        selected?.intelligence_index != null ? ` · quality ${selected.intelligence_index.toFixed(1)}` : ""
                      }`
                    : "Custom OpenRouter model · pricing is verified before use"}
                </div>
              </div>
              <ChevronDown size={15} className="text-text-muted" />
            </button>

            {selectedAssessment && (
              <div
                className={cn(
                  "mt-2 rounded-[var(--radius-m)] border px-2.5 py-2 text-xs leading-4",
                  selectedAssessment.fits
                    ? "border-success/15 bg-success/[0.045] text-text-muted"
                    : "border-amber-500/20 bg-amber-500/[0.07] text-amber-200"
                )}
              >
                <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
                  <span>
                    {selectedAssessment.fits
                      ? "Fits your current cost and context guardrails"
                      : selectedAssessment.reasons.join(" · ")}
                  </span>
                  {meetingsRemaining != null && (
                    <span className="tnum shrink-0 font-mono">
                      ≈ {meetingsRemaining.toLocaleString()} similar summaries left this month
                    </span>
                  )}
                </div>
              </div>
            )}

            {modelOpen && (
              <div className="absolute inset-x-0 top-full z-30 mt-1.5 overflow-hidden rounded-[var(--radius-l)] border border-border-hover bg-surface-1 shadow-[var(--shadow-lg)]">
                <div className="flex items-center gap-2 border-b border-border px-3">
                  <Search size={14} className="text-text-muted" />
                  <input
                    autoFocus
                    value={query}
                    onChange={(event) => setQuery(event.target.value)}
                    placeholder="Search 400+ compatible models"
                    className="h-10 min-w-0 flex-1 bg-transparent text-xs text-text-primary outline-none"
                  />
                </div>

                <div className="grid grid-cols-2 gap-2 border-b border-border bg-surface-0/45 p-2">
                  <div>
                    <span className="eyebrow block">Meeting length</span>
                    <div className="mt-1">
                      <Select
                        aria-label="Meeting length estimate"
                        options={MEETING_LENGTH_OPTIONS.map((minutes) => ({
                          value: String(minutes),
                          label: `${minutes} minutes`,
                        }))}
                        value={String(meetingMinutes)}
                        onChange={(value) => setMeetingMinutes(Number(value))}
                        className={DENSE_CONTROL}
                      />
                    </div>
                  </div>
                  <div>
                    <span className="eyebrow block">Sort by</span>
                    <div className="mt-1">
                      <Select
                        aria-label="Sort models"
                        options={SORT_OPTIONS}
                        value={modelSort}
                        onChange={setModelSort}
                        className={DENSE_CONTROL}
                      />
                    </div>
                  </div>
                  <Checkbox
                    className="col-span-2"
                    checked={onlyWithinLimits}
                    onChange={setOnlyWithinLimits}
                    label={<span className="text-xs">Show only models within my guardrails</span>}
                  />
                </div>

                <div className="max-h-64 overflow-y-auto p-1.5">
                  {loadingModels ? (
                    <div className="flex h-20 items-center justify-center text-text-muted">
                      <Loader2 size={16} className="animate-spin" />
                    </div>
                  ) : (
                    filtered.map((model) => (
                      <ModelOption
                        key={model.id}
                        model={model}
                        minutes={meetingMinutes}
                        assessment={assessModelForMeeting(model, settings, usage, meetingMinutes)}
                        current={settings.model === model.id}
                        onPick={() => {
                          persist({ ...settings, model: model.id });
                          setModelOpen(false);
                          setQuery("");
                        }}
                      />
                    ))
                  )}
                  {!loadingModels && filtered.length === 0 && (
                    <p className="p-4 text-center text-xs text-text-muted">No compatible models found.</p>
                  )}
                </div>

                <div className="border-t border-border p-2">
                  <input
                    value={settings.model}
                    onChange={(event) => setSettings({ ...settings, model: event.target.value })}
                    onBlur={() => persist(settings)}
                    aria-label="Custom OpenRouter model slug"
                    className="h-[var(--control-h-s)] w-full rounded-[var(--radius-m)] border border-border bg-surface-0 px-2.5 font-mono text-xs text-text-secondary outline-none focus:border-amber-500/50"
                  />
                </div>
                <p className="border-t border-border px-3 py-2 text-xs leading-4 text-text-muted">
                  Planning estimates compare models using a typical transcript and summary. Quality is
                  OpenRouter's broad Artificial Analysis intelligence index, not a meeting-specific guarantee.
                  OmniVox recalculates the real maximum cost before every paid request.
                </p>
              </div>
            )}
          </div>

          <button
            onClick={() => setAdvanced((value) => !value)}
            className="mt-3 flex w-full items-center justify-between py-1 text-xs font-medium text-text-secondary transition-colors duration-[var(--dur-2)] ease-out hover:text-text-primary"
          >
            Cost, routing &amp; privacy controls
            <ChevronDown
              size={14}
              className={cn("transition-transform duration-[var(--dur-2)] ease-out", advanced && "rotate-180")}
            />
          </button>

          {advanced && (
            <div className="mt-2 space-y-3 rounded-[var(--radius-l)] border border-border bg-surface-0/50 p-3">
              <AdvancedField
                label="Transcription timing"
                hint="Audio is saved in recoverable 20-second chunks either way. After meeting keeps local Whisper idle while you are on the call."
              >
                <Select
                  aria-label="Transcription timing"
                  options={TRANSCRIPTION_OPTIONS}
                  value={settings.transcription_mode}
                  onChange={(value) => persist({ ...settings, transcription_mode: value })}
                  className={DENSE_CONTROL}
                />
              </AdvancedField>

              <AdvancedField
                label="Provider routing"
                hint="Controls which provider endpoint serves the selected model. Provider fallback, privacy rules, and price ceilings remain enabled."
              >
                <Select
                  aria-label="OpenRouter provider routing"
                  options={ROUTING_OPTIONS}
                  value={settings.routing_preference}
                  onChange={(value) => persist({ ...settings, routing_preference: value })}
                  className={DENSE_CONTROL}
                />
              </AdvancedField>

              <AdvancedField label="Summary style">
                <Select
                  aria-label="Summary style"
                  options={SUMMARY_PRESETS.map(([value, label]) => ({ value, label }))}
                  value={settings.summary_preset}
                  onChange={(value) => persist({ ...settings, summary_preset: value })}
                  className={DENSE_CONTROL}
                />
              </AdvancedField>

              <AdvancedField label="Custom summary instructions">
                <Textarea
                  value={settings.custom_instructions}
                  maxLength={2000}
                  onChange={(event) => setSettings({ ...settings, custom_instructions: event.target.value })}
                  onBlur={() => persist(settings)}
                  aria-label="Custom summary instructions"
                  placeholder="Example: Put risks first and use terse bullets."
                  className="min-h-16 text-xs leading-5"
                />
              </AdvancedField>

              <div className="grid grid-cols-2 gap-2">
                <MoneyInput
                  label="Per meeting"
                  value={settings.per_meeting_budget}
                  onChange={(value) => persist({ ...settings, per_meeting_budget: value })}
                />
                <MoneyInput
                  label="Monthly budget"
                  value={settings.monthly_budget}
                  onChange={(value) => persist({ ...settings, monthly_budget: value })}
                />
                <MoneyInput
                  label="Max input / M"
                  value={settings.max_prompt_price}
                  onChange={(value) => persist({ ...settings, max_prompt_price: value })}
                />
                <MoneyInput
                  label="Max output / M"
                  value={settings.max_completion_price}
                  onChange={(value) => persist({ ...settings, max_completion_price: value })}
                />
              </div>

              <SettingToggle
                icon={ShieldCheck}
                title="Zero retention only"
                checked={settings.zdr_only}
                onChange={(checked) => persist({ ...settings, zdr_only: checked })}
              />
              <SettingToggle
                icon={ShieldCheck}
                title="Deny data-collecting endpoints"
                checked={settings.deny_data_collection}
                onChange={(checked) => persist({ ...settings, deny_data_collection: checked })}
              />
              <SettingToggle
                icon={Sparkles}
                title="Summarize automatically after calls"
                checked={settings.auto_summarize}
                onChange={(checked) => persist({ ...settings, auto_summarize: checked })}
              />
              <SettingToggle
                icon={RefreshCw}
                title="Suggest recording in calls"
                checked={settings.auto_suggest}
                onChange={(checked) => persist({ ...settings, auto_suggest: checked })}
              />
            </div>
          )}

          <div className="mt-4 flex flex-wrap gap-2">
            <Button
              size="sm"
              variant="secondary"
              onClick={() => openOpenRouterCredits().catch((error) => setMessage(String(error)))}
              icon={<ExternalLink />}
            >
              Add credits
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => openOpenRouterKeys().catch((error) => setMessage(String(error)))}
              icon={<KeyRound />}
            >
              Manage key
            </Button>
            <Button size="sm" variant="ghost" onClick={disconnect} disabled={busy} icon={<Trash2 />}>
              Disconnect
            </Button>
          </div>
        </>
      )}

      {message && (
        <div
          className={cn(
            "mt-3 rounded-[var(--radius-m)] px-3 py-2 text-xs",
            message === "Saved" || message.includes("connected") || message.includes("copied")
              ? "bg-success/10 text-success"
              : "bg-amber-500/10 text-amber-200"
          )}
        >
          {message}
        </div>
      )}
    </section>
  );
}

/** Label + control + optional hint — the advanced block's one field shape. */
function AdvancedField({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div>
      <span className="eyebrow block">{label}</span>
      <div className="mt-1">{children}</div>
      {hint && <p className="mt-1.5 text-xs leading-4 text-text-muted/80">{hint}</p>}
    </div>
  );
}

/** One row of the model comparison list. */
function ModelOption({
  model,
  minutes,
  assessment,
  current,
  onPick,
}: {
  model: OpenRouterModel;
  minutes: number;
  assessment: ReturnType<typeof assessModelForMeeting>;
  current: boolean;
  onPick: () => void;
}) {
  return (
    <button
      title={assessment.reasons.join(" · ")}
      onClick={onPick}
      className="flex w-full items-center gap-2 rounded-[var(--radius-m)] px-2.5 py-2 text-left transition-colors duration-[var(--dur-2)] ease-out hover:bg-surface-2"
    >
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5">
          <span className="truncate text-xs font-medium text-text-primary">{model.name}</span>
          <span
            className={cn(
              "shrink-0 rounded-[var(--radius-s)] px-1 py-0.5 text-2xs font-semibold uppercase tracking-wide",
              assessment.fits ? "bg-success/10 text-success" : "bg-amber-500/10 text-amber-200"
            )}
          >
            {assessment.fits ? "Fits" : "Blocked"}
          </span>
          {model.intelligence_index != null && (
            <span
              title="Broad Artificial Analysis intelligence benchmark supplied by OpenRouter"
              className="shrink-0 rounded-[var(--radius-s)] bg-violet-500/10 px-1 py-0.5 text-2xs font-semibold text-violet-300"
            >
              Q {model.intelligence_index.toFixed(1)}
            </span>
          )}
        </div>
        <div className="truncate text-xs text-text-muted">
          {model.id} · {compactTokens(model.context_length)} ctx · {money(model.prompt_price_million)}/M in ·{" "}
          {money(model.completion_price_million)}/M out
        </div>
      </div>
      <div className="shrink-0 text-right">
        <div className="tnum font-mono text-xs font-semibold text-text-primary">
          ≈ {money(assessment.estimatedCost)}
        </div>
        <div className="tnum text-2xs text-text-muted">{minutes} min</div>
      </div>
      {current && <Check size={13} className="shrink-0 text-amber-300" />}
    </button>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-[var(--radius-l)] bg-surface-2 px-3 py-2">
      <div className="text-xs text-text-muted">{label}</div>
      <div className="tnum mt-0.5 font-mono text-xs font-semibold text-text-primary">{value}</div>
    </div>
  );
}

function MoneyInput({ label, value, onChange }: { label: string; value: number; onChange: (value: number) => void }) {
  return (
    <label className="text-xs text-text-muted">
      {label}
      <div className="mt-1 flex h-[var(--control-h-s)] items-center rounded-[var(--radius-m)] border border-border bg-surface-2 px-2">
        <span>$</span>
        <input
          type="number"
          min="0"
          step="0.01"
          value={value}
          onChange={(event) => onChange(Number(event.target.value))}
          className="tnum min-w-0 flex-1 bg-transparent px-1 font-mono text-xs text-text-primary outline-none"
        />
      </div>
    </label>
  );
}

function SettingToggle({
  icon: Icon,
  title,
  checked,
  onChange,
}: {
  icon: typeof ShieldCheck;
  title: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <div className="flex items-center gap-2">
      <Icon size={13} className="text-text-muted" />
      <span className="flex-1 text-xs text-text-secondary">{title}</span>
      <Toggle checked={checked} onChange={onChange} aria-label={title} />
    </div>
  );
}

function UsageLedger({
  records,
  loading,
  onCopy,
}: {
  records: MeetingAiUsageRecord[];
  loading: boolean;
  onCopy: () => void;
}) {
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState("all");
  const [grouped, setGrouped] = useState(true);
  const filtered = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    return records.filter((record) => {
      if (kind !== "all" && record.request_kind !== kind) return false;
      return (
        !normalized
        || `${record.meeting_title ?? "deleted meeting"} ${record.model} ${record.request_kind}`
          .toLowerCase()
          .includes(normalized)
      );
    });
  }, [kind, query, records]);
  const rollups = useMemo(() => rollupUsageByMeeting(filtered), [filtered]);

  return (
    <div className="mt-2 rounded-[var(--radius-l)] border border-border bg-surface-0/55 p-3">
      <div className="flex items-center gap-2">
        <label className="flex h-[var(--control-h-s)] min-w-0 flex-1 items-center gap-2 rounded-[var(--radius-m)] border border-border bg-surface-2 px-2">
          <Search size={11} className="text-text-muted" />
          <input
            aria-label="Search AI activity"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Meeting or model"
            className="min-w-0 flex-1 bg-transparent text-xs text-text-primary outline-none"
          />
        </label>
        <div className="w-36 shrink-0">
          <Select
            aria-label="Filter AI activity"
            options={LEDGER_KIND_OPTIONS}
            value={kind}
            onChange={setKind}
            className={DENSE_CONTROL}
          />
        </div>
      </div>

      <div className="mt-2 flex items-center justify-between">
        <div className="flex rounded-[var(--radius-m)] bg-surface-2 p-0.5">
          {([[true, "By meeting"], [false, "Requests"]] as const).map(([value, label]) => (
            <button
              key={label}
              onClick={() => setGrouped(value)}
              className={cn(
                "rounded-[var(--radius-s)] px-2 py-1 text-xs transition-colors duration-[var(--dur-2)] ease-out",
                grouped === value ? "bg-surface-3 text-text-primary" : "text-text-muted"
              )}
            >
              {label}
            </button>
          ))}
        </div>
        <button
          disabled={records.length === 0}
          onClick={onCopy}
          className="flex items-center gap-1 rounded-[var(--radius-s)] px-2 py-1 text-xs text-text-muted transition-colors duration-[var(--dur-2)] ease-out hover:bg-surface-2 hover:text-text-primary disabled:cursor-not-allowed disabled:opacity-45"
        >
          <Copy size={9} /> Copy CSV
        </button>
      </div>

      <div className="mt-2 max-h-52 overflow-y-auto">
        {loading ? (
          <div className="flex h-20 items-center justify-center text-text-muted">
            <Loader2 size={14} className="animate-spin" />
          </div>
        ) : grouped ? (
          rollups.map((row) => (
            <LedgerRow
              key={row.key}
              title={row.meetingTitle}
              detail={`${row.requests} request${row.requests === 1 ? "" : "s"} · ${(row.promptTokens + row.completionTokens).toLocaleString()} tokens · ${new Date(row.latestAt).toLocaleDateString()}`}
              cost={money(row.cost)}
            />
          ))
        ) : (
          filtered.map((record) => (
            <LedgerRow
              key={record.id}
              kind={record.request_kind}
              title={record.meeting_title ?? "Deleted meeting"}
              detail={`${record.model} · ${(record.prompt_tokens + record.completion_tokens).toLocaleString()} tokens · ${new Date(record.created_at).toLocaleString()}`}
              cost={money(record.cost)}
            />
          ))
        )}
        {!loading && filtered.length === 0 && (
          <p className="py-7 text-center text-xs text-text-muted">No AI activity matches this view.</p>
        )}
      </div>

      <p className="mt-2 border-t border-border pt-2 text-xs leading-4 text-text-muted">
        Actual costs returned by OpenRouter. Deleting a meeting removes its content but keeps an anonymized
        spending row for budget integrity.
      </p>
    </div>
  );
}

/** One ledger line — grouped rollup or single request; the kind chip is the only difference. */
function LedgerRow({
  kind,
  title,
  detail,
  cost,
}: {
  kind?: string;
  title: string;
  detail: string;
  cost: string;
}) {
  return (
    <div className="flex items-center gap-2 border-t border-border/70 py-2 first:border-t-0">
      {kind && (
        <span
          className={cn(
            "rounded-[var(--radius-s)] px-1 py-0.5 text-2xs font-semibold uppercase",
            kind === "question" ? "bg-indigo-500/10 text-indigo-300" : "bg-violet-500/10 text-violet-300"
          )}
        >
          {kind}
        </span>
      )}
      <div className="min-w-0 flex-1">
        <div className="truncate text-xs font-medium text-text-primary">{title}</div>
        <div className="tnum truncate text-xs text-text-muted">{detail}</div>
      </div>
      <span className="tnum shrink-0 font-mono text-xs font-semibold text-text-primary">{cost}</span>
    </div>
  );
}
