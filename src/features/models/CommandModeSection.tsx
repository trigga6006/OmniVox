import { useCallback, useEffect, useRef, useState } from "react";
import { Zap, FlaskConical, ArrowRight, Cpu } from "lucide-react";
import {
  getSettings,
  updateSettings,
  onSettingsChanged,
  getActiveLlmModel,
  onLlmModelLoaded,
  testCommand,
  type AppSettings,
  type CommandTestResult,
} from "@/lib/tauri";
import { Button, Card, Toggle, Badge } from "@/components/ui";

/**
 * Command Mode manager — the third Models tab (alongside Speech & LLM).
 *
 * Command Mode SHARES the active LLM with Structured Mode (one model, two
 * jobs), so this tab has no model list of its own — it owns the command
 * settings + a dry-run "Test command" box that shows how the two-tier brain
 * (instant matcher → LLM fallback) resolves a phrase WITHOUT executing it.
 * Amber accent, matching the command pill.
 */
const EXAMPLES = [
  "open spotify",
  "search for the weather in denver",
  "close this window",
  "show me the desktop",
  "skip this song",
];

export function CommandModeSection() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [modelName, setModelName] = useState<string | null>(null);
  const [utterance, setUtterance] = useState("");
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<CommandTestResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const refreshModel = useCallback(async () => {
    try {
      const active = await getActiveLlmModel();
      if (mountedRef.current) setModelName(active?.name ?? null);
    } catch {
      /* ignore */
    }
  }, []);

  useEffect(() => {
    getSettings()
      .then((s) => {
        if (mountedRef.current) setSettings(s);
      })
      .catch(() => {});
    refreshModel();

    const unlistenSettings = onSettingsChanged((s) => {
      if (mountedRef.current) setSettings(s);
    });
    const unlistenLoaded = onLlmModelLoaded(() => refreshModel());
    return () => {
      unlistenSettings.then((fn) => fn());
      unlistenLoaded.then((fn) => fn());
    };
  }, [refreshModel]);

  const applyPatch = useCallback(
    async (patch: Partial<AppSettings>) => {
      if (!settings) return;
      const updated: AppSettings = { ...settings, ...patch };
      setSettings(updated); // optimistic
      try {
        await updateSettings(updated);
      } catch (e) {
        console.error("Update settings failed:", e);
        try {
          const s = await getSettings();
          if (mountedRef.current) setSettings(s);
        } catch {
          /* ignore */
        }
      }
    },
    [settings]
  );

  const commandMode = settings?.command_mode ?? false;
  const launchAppEnabled = settings?.launch_app_voice_commands_enabled ?? true;

  const handleTest = useCallback(
    async (text?: string) => {
      const phrase = (text ?? utterance).trim();
      if (!phrase || testing) return;
      if (text !== undefined) setUtterance(text);
      setTesting(true);
      setError(null);
      setResult(null);
      try {
        const r = await testCommand(phrase);
        if (mountedRef.current) setResult(r);
      } catch (e) {
        if (mountedRef.current) setError(String(e));
      } finally {
        if (mountedRef.current) setTesting(false);
      }
    },
    [utterance, testing]
  );

  return (
    <section className="flex flex-col gap-3">
      {/* Enable + shared-model note */}
      <Card
        className="px-4 py-3.5 opacity-0 animate-slide-up"
        style={{ animationDelay: "0.05s", animationFillMode: "forwards" }}
      >
        <div className="flex items-center justify-between">
          <div className="min-w-0 flex-1 pr-3">
            <p className="text-[14px] text-text-primary">Enable Command Mode</p>
            <p className="mt-0.5 text-[11.5px] leading-snug text-text-muted">
              Hold <span className="text-amber-300/90">Right Ctrl</span> and speak a command —
              “open Spotify”, “close this window”, “search for…”. Performs an action instead of
              typing.
            </p>
          </div>
          <Toggle
            accent="amber"
            checked={commandMode}
            onChange={() => applyPatch({ command_mode: !commandMode })}
            aria-label="Toggle Command Mode"
          />
        </div>

        {/* Opt-in for CUSTOM "Launch" voice-commands (your own dictation
            commands that start a program). This does NOT affect Command Mode's
            built-in "open <app>" — that always works. */}
        <div className="mt-3.5 flex items-center justify-between border-t border-border/60 pt-3">
          <div className="min-w-0 flex-1 pr-3">
            <p className="text-[14px] text-text-primary">Allow custom Launch voice-commands</p>
            <p className="mt-0.5 text-[11.5px] leading-snug text-text-muted">
              Lets your own “Launch” voice-commands start programs. Command Mode’s built-in
              “open …” isn’t affected. Off = your Launch commands do nothing.
            </p>
          </div>
          <Toggle
            accent="amber"
            checked={launchAppEnabled}
            onChange={() =>
              applyPatch({ launch_app_voice_commands_enabled: !launchAppEnabled })
            }
            aria-label="Allow custom Launch voice-commands"
          />
        </div>

        {/* Shared-model note */}
        <div className="mt-3.5 flex items-center gap-2 border-t border-border/60 pt-3 text-[11.5px] text-text-muted">
          <Cpu size={13} strokeWidth={2} className="shrink-0 text-text-muted/80" />
          <span className="min-w-0">
            Uses your active LLM
            {modelName ? (
              <>
                {" "}
                — <span className="text-text-secondary">{modelName}</span>
              </>
            ) : (
              " (none active yet)"
            )}{" "}
            for phrasings the instant matcher doesn’t catch. It’s shared with Structured Mode —
            change it in the <span className="text-violet-300/90">LLM Structuring</span> tab.
          </span>
        </div>
      </Card>

      {/* Test command box */}
      <Card
        className="px-4 py-3.5 opacity-0 animate-slide-up"
        style={{ animationDelay: "0.09s", animationFillMode: "forwards" }}
      >
        <div className="mb-2 flex items-center gap-1.5">
          <FlaskConical size={12} strokeWidth={2} className="text-text-muted" />
          <span className="text-[11px] font-semibold uppercase tracking-wider text-text-muted">
            Test command
          </span>
        </div>
        <p className="mb-3 text-[11.5px] leading-snug text-text-muted">
          Type a phrase to see how the brain resolves it — nothing is executed.
        </p>

        <div className="flex items-center gap-2">
          <input
            type="text"
            value={utterance}
            onChange={(e) => setUtterance(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleTest();
            }}
            placeholder="e.g. close this window"
            className="min-w-0 flex-1 rounded-lg border border-border bg-surface-2 px-3 py-2 text-[13px] text-text-primary placeholder:text-text-muted/60 focus:border-amber-500/50 focus:outline-none"
          />
          <Button
            size="sm"
            variant="primary"
            onClick={() => handleTest()}
            disabled={!utterance.trim() || testing}
            loading={testing}
            icon={<ArrowRight strokeWidth={2} />}
          >
            Test
          </Button>
        </div>

        {/* Example chips */}
        <div className="mt-2.5 flex flex-wrap gap-1.5">
          {EXAMPLES.map((ex) => (
            <button
              key={ex}
              onClick={() => handleTest(ex)}
              disabled={testing}
              className="rounded-full border border-border bg-surface-2/60 px-2.5 py-1 text-[11px] text-text-muted transition-colors hover:border-amber-500/40 hover:text-text-secondary disabled:opacity-50"
            >
              {ex}
            </button>
          ))}
        </div>

        {/* Result */}
        {result && (
          <div className="mt-3 flex items-center gap-2.5 rounded-lg border border-border bg-surface-2/40 px-3 py-2.5">
            {result.recognized ? (
              <Zap size={14} strokeWidth={2} className="shrink-0 text-amber-400" fill="currentColor" />
            ) : (
              <span className="shrink-0 text-[13px] font-bold text-text-muted">∅</span>
            )}
            <span className="min-w-0 flex-1 truncate text-[13px] text-text-primary">
              {result.summary}
            </span>
            <Badge tone={result.tier === "matcher" ? "green" : result.tier === "llm" ? "violet" : "neutral"}>
              {result.tier === "matcher"
                ? "instant"
                : result.tier === "llm"
                  ? "LLM"
                  : "no match"}
            </Badge>
            <span className="shrink-0 font-mono text-[11px] tabular-nums text-text-muted">
              {result.duration_ms}ms
            </span>
          </div>
        )}
        {error && (
          <div className="mt-3 rounded-lg border border-error/25 bg-error/[0.08] px-3 py-2 text-[11.5px] text-error">
            {error}
          </div>
        )}
      </Card>
    </section>
  );
}
