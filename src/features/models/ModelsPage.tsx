import { useState } from "react";
import { Mic, Sparkles, Terminal } from "lucide-react";
import { Segmented } from "@/components/ui";
import { SpeechModelsSection } from "./SpeechModelsSection";
import { LlmModelsSection } from "./LlmModelsSection";
import { CommandModeSection } from "./CommandModeSection";

type Tab = "speech" | "llm" | "command";

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
    <div className="flex h-full flex-col overflow-y-auto px-8 pt-6 pb-8">
      {/* Header */}
      <div
        className="opacity-0 animate-slide-up"
        style={{ animationDelay: "0.05s", animationFillMode: "forwards" }}
      >
        <h1 className="font-display text-2xl font-semibold tracking-[-0.02em] text-text-primary">
          Models
        </h1>
        <p className="mt-1 text-sm text-text-muted">
          Speech recognition, structured output, and voice commands.
        </p>
      </div>

      {/* Tab bar — speech & LLM catalogs. Per-tab accent (amber / violet)
          lives on the cards' stripes and chips below. */}
      <div
        className="mt-6 opacity-0 animate-slide-up"
        style={{ animationDelay: "0.08s", animationFillMode: "forwards" }}
      >
        <Segmented<Tab>
          options={[
            {
              value: "speech",
              label: "Speech Recognition",
              icon: <Mic strokeWidth={2} />,
            },
            {
              value: "llm",
              label: "LLM Structuring",
              icon: <Sparkles strokeWidth={2} />,
            },
            {
              value: "command",
              label: "Command",
              icon: <Terminal strokeWidth={2} />,
            },
          ]}
          value={tab}
          onChange={setTab}
        />
      </div>

      {/* Active tab content.  The `key` forces a clean remount on
          switch so each section's state and subscriptions reset. */}
      <div className="mt-5">
        {tab === "speech" ? (
          <SpeechModelsSection key="speech" />
        ) : tab === "llm" ? (
          <LlmModelsSection key="llm" />
        ) : (
          <CommandModeSection key="command" />
        )}
      </div>
    </div>
  );
}
