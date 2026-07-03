import { useEffect, useRef } from "react";
import { Check, Mic, Code, Mail, FileText, Terminal, Globe, Briefcase, Heart, Scale } from "lucide-react";
import type { ContextMode } from "@/lib/tauri";
import { showMainWindow } from "@/lib/tauri";
import { Logo } from "@/components/Logo";
import { cn } from "@/lib/utils";

const ICON_MAP: Record<string, typeof Mic> = {
  mic: Mic,
  code: Code,
  mail: Mail,
  "file-text": FileText,
  terminal: Terminal,
  globe: Globe,
  briefcase: Briefcase,
  heart: Heart,
  scale: Scale,
};

const COLOR_MAP: Record<string, string> = {
  amber: "text-amber-400",   // primary
  blue: "text-indigo-400",   // command blue
  green: "text-success",     // sage → green
  purple: "text-violet-400", // structured
  red: "text-recording-400", // recording/error
  cyan: "text-[var(--color-teal)]",
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

  // Close when overlay window loses focus (click on desktop / another window)
  useEffect(() => {
    const handler = () => onClose();
    window.addEventListener("blur", handler);
    return () => window.removeEventListener("blur", handler);
  }, [onClose]);

  return (
    <div ref={ref} className="mode-selector">
      <div className="ms-ring" aria-hidden="true" />

      {/* Kicker */}
      <div className="ms-header">
        <span className="ms-indicator" aria-hidden="true" />
        <span className="ms-kicker">Context Mode</span>
      </div>

      {/* Mode items */}
      <div className="ms-items">
        {modes.map((mode) => {
          const Icon = ICON_MAP[mode.icon] ?? Mic;
          const colorCls = COLOR_MAP[mode.color] ?? "text-amber-400";
          const isActive = mode.id === activeId;

          return (
            <button
              key={mode.id}
              onClick={() => {
                onSelect(mode.id);
                // Intentionally do NOT close — the user wants to browse/try
                // modes while the menu stays open.  Click-outside and ESC
                // still dismiss.
              }}
              className={cn("ms-item", isActive && "ms-item--active")}
            >
              <Icon size={13} className={cn("ms-item-icon", colorCls)} />
              <span className="ms-item-name">{mode.name}</span>
              {isActive && (
                <Check size={11} className="ms-item-check" strokeWidth={2.6} />
              )}
            </button>
          );
        })}
      </div>

      <div className="ms-divider" aria-hidden="true" />

      {/* Open main window */}
      <div className="ms-footer">
        <button
          className="ms-open"
          onClick={() => {
            showMainWindow().catch(() => {});
            onClose();
          }}
        >
          <Logo size={11} className="ms-open-logo" />
          <span className="ms-open-label">Open OmniVox</span>
        </button>
      </div>

      <style>{styles}</style>
    </div>
  );
}

const styles = `
/* ══════════════════════════════════════════════════════════════
   Mode Selector — premium graphite surface. Crisp hairlines, a single
   thin amber accent on the active row. No warm grain, no amber glow.
   ══════════════════════════════════════════════════════════════ */

.mode-selector {
  position: relative;
  width: 196px;
  margin-bottom: 4px;
  border-radius: 13px;
  background: linear-gradient(180deg, #17171b 0%, #100f12 100%);
  border: 1px solid rgba(255,255,255,0.08);
  overflow: hidden;
  isolation: isolate;
  box-shadow:
    inset 0 1px 0 rgba(255,255,255,0.05),
    0 1px 2px rgba(0,0,0,0.5),
    0 10px 24px -8px rgba(0,0,0,0.7),
    0 24px 48px -16px rgba(0,0,0,0.85);
  animation: ms-in 220ms cubic-bezier(0.16, 1, 0.3, 1) both;
  flex-shrink: 0;
}

/* Top hairline highlight */
.ms-ring {
  position: absolute;
  top: 0; left: 0; right: 0;
  height: 1px;
  background: linear-gradient(90deg,
    transparent 0%,
    rgba(255,255,255,0.10) 50%,
    transparent 100%);
  pointer-events: none;
  z-index: 2;
}
.mode-selector > *:not(.ms-ring) {
  position: relative;
  z-index: 3;
}

/* Header kicker ----------------------------------------------- */
.ms-header {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 10px 12px 7px;
}
.ms-indicator {
  display: inline-block;
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--color-amber-500);
}
.ms-kicker {
  font-family: var(--font-mono);
  font-size: 9px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.2em;
  color: var(--color-text-secondary);
}

/* Mode items --------------------------------------------------- */
.ms-items {
  padding: 0 5px 4px;
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.ms-item {
  position: relative;
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 6px 9px;
  border-radius: 8px;
  border: 0;
  background: transparent;
  cursor: pointer;
  text-align: left;
  transition: background 140ms ease, color 140ms ease;
}
.ms-item:hover {
  background: rgba(255,255,255,0.04);
}
.ms-item--active {
  background: rgba(255,255,255,0.05);
}
.ms-item--active::before {
  content: "";
  position: absolute;
  left: 0;
  top: 7px;
  bottom: 7px;
  width: 2px;
  border-radius: 2px;
  background: var(--color-amber-500);
}
.ms-item-icon {
  flex-shrink: 0;
}
.ms-item-name {
  flex: 1;
  font-family: var(--font-sans);
  font-size: 11px;
  font-weight: 500;
  color: rgba(244,244,245,0.78);
  letter-spacing: -0.005em;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ms-item--active .ms-item-name {
  color: var(--color-text-primary);
}
.ms-item-check {
  color: var(--color-amber-400);
  flex-shrink: 0;
}

/* Divider ------------------------------------------------------ */
.ms-divider {
  height: 1px;
  margin: 3px 12px;
  background: linear-gradient(90deg,
    transparent 0%,
    rgba(255,255,255,0.09) 50%,
    transparent 100%);
}

/* Footer open main window ------------------------------------- */
.ms-footer {
  padding: 3px 5px 6px;
}
.ms-open {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 6px 9px;
  border-radius: 8px;
  background: transparent;
  border: 0;
  cursor: pointer;
  text-align: left;
  transition: background 140ms ease;
}
.ms-open:hover {
  background: rgba(255,255,255,0.04);
}
.ms-open-logo {
  opacity: 0.5;
  flex-shrink: 0;
  transition: opacity 140ms ease;
}
.ms-open:hover .ms-open-logo { opacity: 0.85; }
.ms-open-label {
  font-family: var(--font-sans);
  font-size: 10px;
  font-weight: 500;
  color: var(--color-text-muted);
  letter-spacing: -0.005em;
  transition: color 140ms ease;
}
.ms-open:hover .ms-open-label {
  color: var(--color-text-secondary);
}

/* Keyframes ---------------------------------------------------- */
@keyframes ms-in {
  from {
    opacity: 0;
    transform: translateY(6px) scale(0.97);
    filter: blur(3px);
  }
  to {
    opacity: 1;
    transform: translateY(0) scale(1);
    filter: blur(0);
  }
}
`;
