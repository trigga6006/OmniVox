import { useState } from "react";
import { Mic, Sparkles } from "lucide-react";
import { cn } from "@/lib/utils";
import { SpeechModelsSection } from "./SpeechModelsSection";
import { LlmModelsSection } from "./LlmModelsSection";

type Tab = "speech" | "llm";

/**
 * Models page — a tabbed catalog.
 *
 * Tab 1 (Speech Recognition) lists Whisper ASR models.
 * Tab 2 (LLM Structuring) lists the Structured-Mode language models
 * plus the compact config strip (min chars, LLM timeout, test button).
 *
 * Keeping both tabs mounted would double the `listModels` / `listLlmModels`
 * traffic on every page visit and make the download-progress effects
 * race with each other.  We mount only the active tab and re-mount on
 * switch so each section owns its lifecycle cleanly.  Users rarely
 * switch tabs mid-download; the single-tab trade is the right call.
 */
export function ModelsPage() {
  const [tab, setTab] = useState<Tab>("speech");

  return (
    <div className="flex h-full flex-col overflow-y-auto px-8 py-8">
      {/* Header */}
      <div
        className="opacity-0 animate-slide-up"
        style={{ animationDelay: "0.05s", animationFillMode: "forwards" }}
      >
        <h1 className="font-display text-2xl font-semibold tracking-[-0.02em] text-text-primary">
          Models
        </h1>
        <p className="mt-1 text-sm text-text-muted">
          Speech recognition and structured-output language models.
        </p>
      </div>

      {/* Tab bar — amber for speech (matches the Whisper accent),
          violet for LLM (matches the Structured Mode accent). */}
      <div
        className="mt-6 flex items-center gap-1 border-b border-border/70 opacity-0 animate-slide-up"
        style={{ animationDelay: "0.08s", animationFillMode: "forwards" }}
        role="tablist"
        aria-label="Model catalog"
      >
        <TabButton
          label="Speech Recognition"
          icon={<Mic size={14} strokeWidth={2} />}
          active={tab === "speech"}
          accent="amber"
          onClick={() => setTab("speech")}
        />
        <TabButton
          label="LLM Structuring"
          icon={<Sparkles size={14} strokeWidth={2} />}
          active={tab === "llm"}
          accent="violet"
          onClick={() => setTab("llm")}
        />
      </div>

      {/* Active tab content.  The `key` forces a clean remount on
          switch so each section's state and subscriptions reset. */}
      <div className="mt-5">
        {tab === "speech" ? (
          <SpeechModelsSection key="speech" />
        ) : (
          <LlmModelsSection key="llm" />
        )}
      </div>
    </div>
  );
}

function TabButton({
  label,
  icon,
  active,
  accent,
  onClick,
}: {
  label: string;
  icon: React.ReactNode;
  active: boolean;
  accent: "amber" | "violet";
  onClick: () => void;
}) {
  const activeText = accent === "amber" ? "text-amber-300" : "text-violet-300";
  const activeUnderline =
    accent === "amber" ? "bg-amber-400" : "bg-violet-400";
  return (
    <button
      onClick={onClick}
      role="tab"
      aria-selected={active}
      className={cn(
        "relative flex items-center gap-2 px-3.5 py-3 text-sm font-medium transition-colors",
        active ? activeText : "text-text-muted hover:text-text-secondary"
      )}
    >
      {icon}
      <span>{label}</span>
      <span
        className={cn(
          "absolute bottom-[-1px] left-0 right-0 h-[2px] rounded-full transition-opacity",
          activeUnderline,
          active ? "opacity-100" : "opacity-0"
        )}
      />
    </button>
  );
}
