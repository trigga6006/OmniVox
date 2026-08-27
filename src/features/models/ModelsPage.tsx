import { useState, type ReactNode } from "react";
import { Mic, Sparkles, Terminal, Wand2 } from "lucide-react";
import { PageHeader, Tabs } from "@/components/ui";
import { SpeechModelsSection } from "./SpeechModelsSection";
import { LlmModelsSection } from "./LlmModelsSection";
import { CommandModeSection } from "./CommandModeSection";
import { CleanupModelSection } from "./CleanupModelSection";

type Tab = "speech" | "llm" | "command" | "cleanup";

/**
 * Models page — a tabbed catalog.
 *
 * Tab 1 (Speech Recognition) lists Whisper ASR models.
 * Tab 2 (LLM Structuring) lists the Structured-Mode language models
 * plus the compact config strip (min chars, LLM timeout, test button).
 *
 * All four sections stay MOUNTED; switching tabs only flips visibility.
 * They used to mount one at a time and re-mount on every switch, which threw
 * away in-flight download state: leaving a tab mid-download lost the progress
 * bar, and coming back showed a plain "Download" button again — one click from
 * re-fetching a file already on disk. Arrow-key navigation on the tab row put
 * that a single keystroke away.
 *
 * The cost is one extra `listModels` / `listLlmModels` per page visit; the
 * sections are self-refreshing (`onModelLoaded`, `onLlmDownloadProgress`,
 * `onSettingsChanged`), so nothing goes stale for not being re-mounted.
 */
export function ModelsPage() {
  const [tab, setTab] = useState<Tab>("speech");

  return (
    <div className="flex h-full flex-col overflow-y-auto px-8 pt-6 pb-8">
      {/* Header */}
      <PageHeader
        title="Models"
        subtitle="Speech recognition, structured output, and voice commands."
        className="opacity-0 animate-slide-up"
        style={{ animationDelay: "0.05s", animationFillMode: "forwards" }}
      />

      {/* Tab bar — speech & LLM catalogs. Per-tab accent (amber / violet)
          lives on the cards' stripes and chips below. */}
      <div
        className="mt-6 opacity-0 animate-slide-up"
        style={{ animationDelay: "0.08s", animationFillMode: "forwards" }}
      >
        <Tabs<Tab>
          idPrefix="models"
          items={[
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
            {
              value: "cleanup",
              label: "Cleanup",
              icon: <Wand2 strokeWidth={2} />,
            },
          ]}
          value={tab}
          onChange={setTab}
        />
      </div>

      {/* Every panel is mounted; only the active one is displayed. */}
      <div className="mt-5">
        <ModelsTabPanel value="speech" active={tab}>
          <SpeechModelsSection />
        </ModelsTabPanel>
        <ModelsTabPanel value="llm" active={tab}>
          <LlmModelsSection />
        </ModelsTabPanel>
        <ModelsTabPanel value="command" active={tab}>
          <CommandModeSection />
        </ModelsTabPanel>
        <ModelsTabPanel value="cleanup" active={tab}>
          <CleanupModelSection />
        </ModelsTabPanel>
      </div>
    </div>
  );
}

/**
 * One always-mounted tab panel. `hidden` (not conditional rendering) is what
 * keeps a background download's state alive across a tab switch.
 *
 * No crossfade: a CSS transition can't run on a `display: none` swap, and
 * keeping the outgoing panel in the layout to fade it out would push the
 * incoming one down the page. Instant swap between mounted panels — correct
 * state beats the fade.
 */
function ModelsTabPanel({
  value,
  active,
  children,
}: {
  value: Tab;
  active: Tab;
  children: ReactNode;
}) {
  return (
    <div
      role="tabpanel"
      id={`models-${value}-panel`}
      aria-labelledby={`models-${value}`}
      hidden={value !== active}
    >
      {children}
    </div>
  );
}
