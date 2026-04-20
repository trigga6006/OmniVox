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

  // Close when overlay window loses focus (click on desktop / another window)
  useEffect(() => {
    const handler = () => onClose();
    window.addEventListener("blur", handler);
    return () => window.removeEventListener("blur", handler);
  }, [onClose]);

  return (
    <div ref={ref} className="mode-selector">
      <div className="ms-bloom" aria-hidden="true" />
      <div className="ms-grain" aria-hidden="true" />
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
   Mode Selector — refined premium surface, amber signature.
   Same surface language as StructuredPanel, tuned to the app's
   primary Studio-Monitor accent (amber) instead of violet.
   ══════════════════════════════════════════════════════════════ */

.mode-selector {
  position: relative;
  width: 192px;
  margin-bottom: 4px;
  border-radius: 12px;
  background:
    linear-gradient(180deg,
      rgba(28,26,24,1) 0%,
      rgba(22,21,20,1) 100%);
  border: 1px solid rgba(255,255,255,0.055);
  overflow: hidden;
  isolation: isolate;
  box-shadow:
    inset 0 1px 0 rgba(255,255,255,0.05),
    0 1px 2px rgba(0,0,0,0.5),
    0 8px 20px -6px rgba(0,0,0,0.7),
    0 20px 40px -14px rgba(0,0,0,0.85);
  animation: ms-in 220ms cubic-bezier(0.16, 1, 0.3, 1) both;
  flex-shrink: 0;
}

/* Atmosphere --------------------------------------------------- */
.ms-bloom {
  position: absolute;
  inset: -70% -20% auto -20%;
  height: 160px;
  background:
    radial-gradient(ellipse at 50% 0%,
      rgba(255,232,185,0.04) 0%,
      rgba(255,222,168,0.02) 28%,
      transparent 60%);
  pointer-events: none;
  z-index: 0;
}
.ms-ring {
  position: absolute;
  top: 0; left: 0; right: 0;
  height: 1px;
  background: linear-gradient(90deg,
    transparent 0%,
    rgba(255,228,190,0.08) 25%,
    rgba(255,234,204,0.14) 50%,
    rgba(255,228,190,0.08) 75%,
    transparent 100%);
  pointer-events: none;
  z-index: 2;
}
.ms-grain {
  position: absolute;
  inset: 0;
  background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='160' height='160'><filter id='n'><feTurbulence type='fractalNoise' baseFrequency='0.85' numOctaves='2' stitchTiles='stitch'/><feColorMatrix values='0 0 0 0 0.7  0 0 0 0 0.62  0 0 0 0 0.55  0 0 0 0.05 0'/></filter><rect width='100%' height='100%' filter='url(%23n)'/></svg>");
  opacity: 0.4;
  mix-blend-mode: overlay;
  pointer-events: none;
  z-index: 1;
}
.mode-selector > *:not(.ms-bloom):not(.ms-grain):not(.ms-ring) {
  position: relative;
  z-index: 3;
}

/* Header kicker ----------------------------------------------- */
.ms-header {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 9px 12px 7px;
}
.ms-indicator {
  display: inline-block;
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: rgb(244,190,110);
  box-shadow:
    0 0 0 1.5px rgba(244,190,110,0.22),
    0 0 6px rgba(244,190,110,0.5);
  animation: ms-indicator-breathe 2.8s ease-in-out infinite;
}
.ms-kicker {
  font-family: var(--font-display);
  font-size: 9px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.22em;
  color: rgba(240,218,182,0.88);
}

/* Mode items --------------------------------------------------- */
.ms-items {
  padding: 0 5px 4px;
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.ms-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 5.5px 8px;
  border-radius: 7px;
  border: 1px solid transparent;
  background: transparent;
  cursor: pointer;
  text-align: left;
  transition:
    background 140ms ease,
    border-color 140ms ease,
    color 140ms ease;
}
.ms-item:hover {
  background: rgba(255,255,255,0.035);
}
.ms-item--active {
  background: rgba(232,180,95,0.08);
  border-color: rgba(232,180,95,0.18);
  box-shadow:
    inset 0 1px 0 rgba(255,230,190,0.04);
}
.ms-item--active:hover {
  background: rgba(232,180,95,0.12);
  border-color: rgba(232,180,95,0.26);
}
.ms-item-icon {
  flex-shrink: 0;
  filter: drop-shadow(0 0 2px rgba(0,0,0,0.2));
}
.ms-item-name {
  flex: 1;
  font-family: var(--font-sans);
  font-size: 11px;
  font-weight: 500;
  color: rgba(232,228,240,0.8);
  letter-spacing: -0.005em;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ms-item--active .ms-item-name {
  color: rgba(250,232,198,0.96);
}
.ms-item-check {
  color: rgba(244,190,110,0.95);
  flex-shrink: 0;
  filter: drop-shadow(0 0 3px rgba(244,190,110,0.4));
}

/* Divider ------------------------------------------------------ */
.ms-divider {
  height: 1px;
  margin: 2px 12px;
  background: linear-gradient(90deg,
    transparent 0%,
    rgba(255,255,255,0.09) 28%,
    rgba(255,235,200,0.12) 50%,
    rgba(255,255,255,0.09) 72%,
    transparent 100%);
}

/* Footer open main window ------------------------------------- */
.ms-footer {
  padding: 3px 5px 5px;
}
.ms-open {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 5.5px 8px;
  border-radius: 7px;
  background: transparent;
  border: 1px solid transparent;
  cursor: pointer;
  text-align: left;
  transition:
    background 140ms ease,
    border-color 140ms ease;
}
.ms-open:hover {
  background: rgba(255,255,255,0.04);
  border-color: rgba(255,255,255,0.05);
}
.ms-open-logo {
  opacity: 0.45;
  flex-shrink: 0;
  transition: opacity 140ms ease;
}
.ms-open:hover .ms-open-logo { opacity: 0.8; }
.ms-open-label {
  font-family: var(--font-sans);
  font-size: 10px;
  font-weight: 500;
  color: rgba(255,255,255,0.42);
  letter-spacing: -0.005em;
  transition: color 140ms ease;
}
.ms-open:hover .ms-open-label {
  color: rgba(255,235,205,0.82);
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
@keyframes ms-indicator-breathe {
  0%, 100% {
    opacity: 0.8;
    box-shadow:
      0 0 0 1.5px rgba(244,190,110,0.22),
      0 0 5px rgba(244,190,110,0.45);
  }
  50% {
    opacity: 1;
    box-shadow:
      0 0 0 2.5px rgba(244,190,110,0.32),
      0 0 12px rgba(244,190,110,0.72);
  }
}
`;
