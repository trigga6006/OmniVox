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
    <div className="flex h-full flex-col p-6 overflow-y-auto">
      {/* Header */}
      <div
        className="opacity-0 animate-slide-up"
        style={{ animationDelay: "0.05s", animationFillMode: "forwards" }}
      >
        <h1 className="font-display font-semibold text-2xl text-text-primary">
          Models
        </h1>
        <p className="text-sm text-text-muted mt-1">
          Speech recognition and structured-output language models.
        </p>
      </div>

      {/* Tab bar — underline-style tabs below the header.  Amber for
          speech (matches the Whisper accent), violet for LLM (matches
          the Structured Mode accent), so the active-tab underline
          reinforces which catalog you're looking at. */}
      <div
        className="mt-5 flex items-center gap-1 border-b border-border opacity-0 animate-slide-up"
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
  const activeText = accent === "amber" ? "text-amber-400" : "text-violet-300";
  const activeUnderline =
    accent === "amber" ? "bg-amber-400/70" : "bg-violet-400/70";
  return (
    <button
      onClick={onClick}
      role="tab"
      aria-selected={active}
      className={cn(
        "relative flex items-center gap-2 px-4 py-2.5 text-sm font-medium transition-colors",
        active
          ? activeText
          : "text-text-muted hover:text-text-secondary"
      )}
    >
      {icon}
      <span>{label}</span>
      {/* Underline indicator — rendered inside the button so its width
          hugs the label + icon rather than spanning the whole
          flex-item with padding.  1.5 px + translate(0,1px) so it
          sits flush on the border line instead of floating above it. */}
      <span
        className={cn(
          "absolute left-0 right-0 bottom-0 h-[2px] rounded-t-sm translate-y-[1px] transition-opacity",
          activeUnderline,
          active ? "opacity-100" : "opacity-0"
        )}
      />
    </button>
  );
}
