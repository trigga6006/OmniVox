import { useState } from "react";
import { BookOpen } from "lucide-react";

const tabs = ["Words", "Snippets"] as const;
type Tab = (typeof tabs)[number];

export function DictionaryPage() {
  const [activeTab, setActiveTab] = useState<Tab>("Words");

  return (
    <div className="flex h-full flex-col p-6">
      {/* Header */}
      <div
        className="animate-slide-up"
        style={{ opacity: 0, animationDelay: "0.05s", animationFillMode: "forwards" }}
      >
        <h1 className="font-display text-2xl text-text-primary">Dictionary</h1>
        <p className="text-sm text-text-muted mt-1">
          Custom vocabulary & text snippets
        </p>
      </div>

      {/* Tabs */}
      <div
        className="mt-5 flex gap-6 border-b border-border animate-slide-up"
        style={{ opacity: 0, animationDelay: "0.1s", animationFillMode: "forwards" }}
      >
        {tabs.map((tab) => {
          const isActive = activeTab === tab;
          return (
            <button
              key={tab}
              onClick={() => setActiveTab(tab)}
              className={`relative pb-2.5 text-sm font-medium transition-colors ${
                isActive
                  ? "text-amber-400"
                  : "text-text-muted hover:text-text-secondary"
              }`}
            >
              {tab}
              {isActive && (
                <span className="absolute bottom-0 left-0 right-0 h-[2px] rounded-full bg-amber-500" />
              )}
            </button>
          );
        })}
      </div>

      {/* Empty state */}
      <div
        className="flex flex-1 flex-col items-center justify-center gap-4 animate-slide-up"
        style={{ opacity: 0, animationDelay: "0.2s", animationFillMode: "forwards" }}
      >
        {/* Icon with amber ring */}
        <div className="relative flex items-center justify-center">
          <div className="absolute h-20 w-20 rounded-full bg-amber-500/5 border border-amber-700/20" />
          <BookOpen size={40} strokeWidth={1.5} className="relative text-text-muted" />
        </div>

        <div className="text-center mt-2">
          <p className="text-sm font-medium text-text-secondary">
            No custom entries
          </p>
          <p className="text-xs text-text-muted mt-1">
            Add words and replacements to improve transcription accuracy
          </p>
        </div>
      </div>
    </div>
  );
}
