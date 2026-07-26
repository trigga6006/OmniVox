import type { Dispatch, SetStateAction } from "react";
import {
  Eye,
  ShieldCheck,
  Layers,
  Rocket,
  Ghost,
  Send,
  Mic,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { StructuredModeToggle } from "./StructuredModeToggle";

interface QuickTogglesProps {
  autoSwitchModes: boolean;
  livePreviewEnabled: boolean;
  noiseReduction: boolean;
  shipMode: boolean;
  commandSend: boolean;
  ghostMode: boolean;
  structuredMode: boolean;
  structuredVoiceCommand: boolean;
  // Popup visibility lives in FloatingPill: it must survive this component
  // unmounting/remounting (showContent flickers during window resizes, and
  // the Command Mode takeover swaps the whole tree), and the
  // structured-output-ready handler closes the ship popup from outside.
  showShipPopup: boolean;
  showLeyLinePopup: boolean;
  setShowShipPopup: Dispatch<SetStateAction<boolean>>;
  setShowLeyLinePopup: Dispatch<SetStateAction<boolean>>;
  onToggleAutoSwitch: () => void;
  onToggleLivePreview: () => void;
  onToggleNoiseReduction: () => void;
  onToggleShipMode: () => void;
  onToggleCommandSend: () => void;
  onToggleGhostMode: () => void;
  onToggleStructuredMode: () => void;
  onToggleStructuredVoiceCommand: () => void;
}

// The quick-toggle column rendered to the right of the ModeSelector menu.
// Positioned absolutely against FloatingPill's relative wrapper div, so it
// must be rendered as a sibling of ModeSelector inside that wrapper.
export function QuickToggles({
  autoSwitchModes,
  livePreviewEnabled,
  noiseReduction,
  shipMode,
  commandSend,
  ghostMode,
  structuredMode,
  structuredVoiceCommand,
  showShipPopup,
  showLeyLinePopup,
  setShowShipPopup,
  setShowLeyLinePopup,
  onToggleAutoSwitch,
  onToggleLivePreview,
  onToggleNoiseReduction,
  onToggleShipMode,
  onToggleCommandSend,
  onToggleGhostMode,
  onToggleStructuredMode,
  onToggleStructuredVoiceCommand,
}: QuickTogglesProps) {
  return (
    <>
      {/* Right-side controls — Ley Line on top (flagship) then the
          quick-toggle settings circles, all in one flex column so the
          same `gap-1.5` (6 px) spacing rule applies between every
          pair.  Pinned at `top: 0` so the Ley Line's top edge is
          always flush with the ModeSelector's top; the column's
          bottom floats based on content which keeps spacing uniform.
          `items-center` centres the 28 px Ley Line against the 26 px
          circles below it.  Uses mousedown for toggle action since
          the overlay is transparent and click events can be
          swallowed at window edges in WebView2. */}
      <div
        className="absolute flex flex-col items-center gap-1.5"
        style={{
          left: "calc(50% + 96px + 6px)",
          top: "0",
        }}
      >
        <div className="relative">
          <StructuredModeToggle
            active={structuredMode}
            onToggle={onToggleStructuredMode}
            onContextMenu={() => {
              setShowLeyLinePopup((prev) => !prev);
              setShowShipPopup(false);
            }}
          />

          {/* ── Ley Line right-click popup: Voice Command gate ──
              Mirrors the ship button's Command-Send popup, including
              the same right-side position so it never overlays the
              mode selector.  Width math (mirrors the comment on
              ".ship-popup"): button ends at 50%+96+6+28 = 50%+130,
              popup adds 8 gap + 160 = 50%+298 right edge, fits in
              the 600 px window with 2 px margin. */}
          <div
            className="ley-line-popup"
            style={{
              left: "calc(100% + 8px)",
              // Align the popup's top with the button's top instead of
              // centring on the button — the Ley Line is pinned to the
              // menu's TOP edge, so a vertically-centred popup extended
              // above the window and got clipped.  Top-aligned means the
              // popup grows downward from the button into the menu's
              // right margin, always fully visible.
              top: "0",
              transform: `scale(${showLeyLinePopup ? 1 : 0.92})`,
              minWidth: 160,
              opacity: showLeyLinePopup ? 1 : 0,
              pointerEvents: showLeyLinePopup ? "auto" : "none",
            }}
            onMouseDown={(e) => e.stopPropagation()}
          >
            <div className="ley-line-popup-bloom" aria-hidden="true" />
            <div className="ley-line-popup-ring" aria-hidden="true" />
            <div className="ley-line-popup-content">
              <div className="ley-line-popup-header">
                <Mic size={10} strokeWidth={2} className="ley-line-popup-icon" />
                <span className="ley-line-popup-kicker">Voice Command</span>
              </div>
              <p className="ley-line-popup-desc">
                Say “Voxify” at the end to structure — otherwise paste plain
              </p>
              <div className="ley-line-popup-row">
                <button
                  onMouseDown={(e) => {
                    e.stopPropagation();
                    e.preventDefault();
                    onToggleStructuredVoiceCommand();
                  }}
                  className={cn(
                    "ley-line-popup-switch",
                    structuredVoiceCommand && "ley-line-popup-switch--on"
                  )}
                >
                  <span className="ley-line-popup-knob" />
                </button>
                <span className="ley-line-popup-state">
                  {structuredVoiceCommand ? "On" : "Off"}
                </span>
              </div>
            </div>
          </div>
        </div>
        <button
          onMouseDown={(e) => {
            e.stopPropagation();
            e.preventDefault();
            onToggleAutoSwitch();
          }}
          data-tip="Auto-switch mode by active app"
          aria-label="Auto-switch mode by active app"
          className={cn(
            "quick-toggle",
            autoSwitchModes && "quick-toggle--on"
          )}
        >
          <Layers size={12} strokeWidth={2} className="quick-toggle-icon" />
        </button>
        <button
          onMouseDown={(e) => {
            e.stopPropagation();
            e.preventDefault();
            onToggleLivePreview();
          }}
          data-tip="Show words live as you speak"
          aria-label="Show words live as you speak"
          className={cn(
            "quick-toggle",
            livePreviewEnabled && "quick-toggle--on"
          )}
        >
          <Eye size={12} strokeWidth={2} className="quick-toggle-icon" />
        </button>
        <button
          onMouseDown={(e) => {
            e.stopPropagation();
            e.preventDefault();
            onToggleNoiseReduction();
          }}
          data-tip="Suppress background noise"
          aria-label="Suppress background noise"
          className={cn(
            "quick-toggle",
            noiseReduction && "quick-toggle--on"
          )}
        >
          <ShieldCheck size={12} strokeWidth={2} className="quick-toggle-icon" />
        </button>
        <div className="relative">
          <button
            onMouseDown={(e) => {
              e.stopPropagation();
              e.preventDefault();
              // Only toggle ship mode on left-click (button 0)
              if (e.button === 0) onToggleShipMode();
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              setShowShipPopup((prev) => !prev);
            }}
            title={shipMode ? "Ship mode: on (right-click for options)" : "Ship mode: off (right-click for options)"}
            className={cn(
              "quick-toggle",
              shipMode && "quick-toggle--on"
            )}
          >
            <Rocket size={12} strokeWidth={2} className="quick-toggle-icon" />
          </button>

          {/* ── Ship button right-click popup ── */}
          <div
            className="ship-popup"
            style={{
              left: "calc(100% + 8px)",
              top: "50%",
              transform: `translateY(-50%) scale(${showShipPopup ? 1 : 0.92})`,
              minWidth: 168,
              opacity: showShipPopup ? 1 : 0,
              pointerEvents: showShipPopup ? "auto" : "none",
            }}
            onMouseDown={(e) => e.stopPropagation()}
          >
            <div className="ship-popup-bloom" aria-hidden="true" />
            <div className="ship-popup-ring" aria-hidden="true" />
            <div className="ship-popup-content">
              <div className="ship-popup-header">
                <Send size={10} strokeWidth={2} className="ship-popup-icon" />
                <span className="ship-popup-kicker">Command Send</span>
              </div>
              <p className="ship-popup-desc">
                Say "send" to submit instead of auto-sending everything
              </p>
              <div className="ship-popup-row">
                <button
                  onMouseDown={(e) => {
                    e.stopPropagation();
                    e.preventDefault();
                    onToggleCommandSend();
                  }}
                  className={cn(
                    "ship-popup-switch",
                    commandSend && "ship-popup-switch--on"
                  )}
                >
                  <span className="ship-popup-knob" />
                </button>
                <span className="ship-popup-state">
                  {commandSend ? "On" : "Off"}
                </span>
              </div>
            </div>
          </div>
        </div>
      </div>
      {/* Divider between toggle buttons and ghost mode */}
      <div
        className="absolute"
        style={{
          left: "calc(50% + 96px + 8px)",
          bottom: "38px",
          width: 22,
          height: 1,
          background:
            "linear-gradient(90deg, rgba(255,235,200,0) 0%, rgba(255,235,200,0.18) 50%, rgba(255,235,200,0) 100%)",
          borderRadius: 1,
        }}
      />
      {/* Ghost mode — positioned parallel with "Open OmniVox" row */}
      <div
        className="absolute"
        style={{
          left: "calc(50% + 96px + 6px)",
          bottom: "8px",
        }}
      >
        <button
          onMouseDown={(e) => {
            e.stopPropagation();
            e.preventDefault();
            onToggleGhostMode();
          }}
          data-tip="Hide the pill until you summon it"
          aria-label="Hide the pill until you summon it"
          className={cn("quick-toggle quick-toggle--ghost", ghostMode && "quick-toggle--ghost-on")}
        >
          <Ghost size={12} strokeWidth={2} className="quick-toggle-icon" />
        </button>
      </div>
    </>
  );
}
