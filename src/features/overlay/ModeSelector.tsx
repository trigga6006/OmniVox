import { useEffect, useRef } from "react";
import { Check, Mic, Code, Mail, FileText, Terminal, Globe } from "lucide-react";
import type { ContextMode } from "@/lib/tauri";
import { cn } from "@/lib/utils";

const ICON_MAP: Record<string, typeof Mic> = {
  mic: Mic,
  code: Code,
  mail: Mail,
  "file-text": FileText,
  terminal: Terminal,
  globe: Globe,
};

const COLOR_MAP: Record<string, string> = {
  amber: "text-amber-400",
  blue: "text-blue-400",
  green: "text-emerald-400",
  purple: "text-purple-400",
  red: "text-red-400",
  cyan: "text-cyan-400",
};

export function ModeSelector({
  modes,
  activeId,
  onSelect,
  onClose,
}: {
  modes: ContextMode[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);

  // Close on click outside
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        onClose();
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [onClose]);

  // Close on Escape
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [onClose]);

  return (
    <div
      ref={ref}
      className="w-48 mb-1 rounded-xl bg-[var(--color-pill-bg)] border border-white/10 shadow-2xl overflow-hidden shrink-0"
      style={{
        animation: "mode-selector-in 0.15s ease-out",
      }}
    >
      <div className="px-2 py-1.5">
        <span className="text-[9px] font-medium text-white/30 uppercase tracking-widest px-2">
          Context Mode
        </span>
      </div>
      <div className="px-1 pb-1 space-y-px">
        {modes.map((mode) => {
          const Icon = ICON_MAP[mode.icon] ?? Mic;
          const colorCls = COLOR_MAP[mode.color] ?? "text-amber-400";
          const isActive = mode.id === activeId;

          return (
            <button
              key={mode.id}
              onClick={() => {
                onSelect(mode.id);
                onClose();
              }}
              className={cn(
                "flex items-center gap-2 w-full rounded-lg px-2 py-1.5 text-left transition-colors",
                isActive
                  ? "bg-white/[0.06]"
                  : "hover:bg-white/[0.04]"
              )}
            >
              <Icon size={13} className={colorCls} />
              <span className="flex-1 text-[11px] font-medium text-white/80 truncate">
                {mode.name}
              </span>
              {isActive && (
                <Check size={11} className="text-amber-400 shrink-0" />
              )}
            </button>
          );
        })}
      </div>

      <style>{`
        @keyframes mode-selector-in {
          from {
            opacity: 0;
            transform: translateY(4px) scale(0.95);
          }
          to {
            opacity: 1;
            transform: translateY(0) scale(1);
          }
        }
      `}</style>
    </div>
  );
}
