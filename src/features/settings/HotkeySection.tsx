import { useEffect, useRef, useState } from "react";
import { RotateCcw } from "lucide-react";
import { suspendHotkey, updateHotkey, type HotkeyConfig } from "@/lib/tauri";
import { CODE_TO_VK } from "@/lib/vk-codes";
import { cn } from "@/lib/utils";
import { Button, Card, Kbd } from "@/components/ui";

/* ─────────────────── Hotkey Recorder Component ─────────────────── */

type HotkeyState = "display" | "listening" | "confirming";

interface CapturedKey {
  code: string;
  vk: number;
  label: string;
}

export function HotkeySection({
  hotkey,
  onSaved,
}: {
  hotkey: HotkeyConfig | null;
  onSaved: (config: HotkeyConfig) => void;
}) {
  const [state, setState] = useState<HotkeyState>("display");
  const [captured, setCaptured] = useState<CapturedKey[]>([]);
  const heldRef = useRef<Set<string>>(new Set());

  const currentLabels = hotkey?.labels ?? ["LCtrl", "LAlt"];

  // ── Listening mode: capture keys ──────────────────────────
  useEffect(() => {
    if (state !== "listening") return;

    // Suspend the backend hook so our keypresses don't trigger recording
    suspendHotkey(true).catch(console.error);

    const collected: CapturedKey[] = [];
    heldRef.current = new Set();

    const onDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      const code = e.code;
      if (heldRef.current.has(code)) return; // repeat
      heldRef.current.add(code);

      const entry = CODE_TO_VK[code];
      if (!entry) return; // unknown key

      // Max 2 keys
      if (collected.length < 2 && !collected.some((k) => k.code === code)) {
        collected.push({ code, vk: entry.vk, label: entry.label });
        setCaptured([...collected]);
      }
    };

    const onUp = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      heldRef.current.delete(e.code);

      // All keys released → done capturing
      if (heldRef.current.size === 0 && collected.length > 0) {
        setState("confirming");
      }
    };

    window.addEventListener("keydown", onDown, true);
    window.addEventListener("keyup", onUp, true);

    return () => {
      window.removeEventListener("keydown", onDown, true);
      window.removeEventListener("keyup", onUp, true);
    };
  }, [state]);

  // If this section unmounts mid-remap (user navigates away while listening or
  // confirming), make sure the global hotkey hook isn't left suspended — that
  // would kill push-to-talk app-wide until restart.  A no-op if not suspended.
  useEffect(() => {
    return () => {
      suspendHotkey(false).catch(() => {});
    };
  }, []);

  const handleRemap = () => {
    setCaptured([]);
    setState("listening");
  };

  const handleRedo = () => {
    setCaptured([]);
    setState("listening");
  };

  const handleCancel = () => {
    setState("display");
    setCaptured([]);
    suspendHotkey(false).catch(console.error);
  };

  const handleSave = () => {
    if (captured.length === 0) return;
    const config: HotkeyConfig = {
      keys: captured.map((k) => k.vk),
      labels: captured.map((k) => k.label),
    };
    updateHotkey(config)
      .then(() => {
        onSaved(config);
        setState("display");
      })
      .catch(console.error);
  };

  const handleReset = () => {
    const config: HotkeyConfig = { keys: [0xa2, 0xa4], labels: ["LCtrl", "LAlt"] };
    updateHotkey(config)
      .then(() => onSaved(config))
      .catch(console.error);
  };

  const isDefault =
    currentLabels.length === 2 &&
    currentLabels[0] === "LCtrl" &&
    currentLabels[1] === "LAlt";

  return (
    <Card
      className={cn(
        "animate-slide-up p-5 transition-colors",
        state === "listening"
          ? "border-amber-400/45 bg-amber-500/[0.03]"
          : state === "confirming"
            ? "border-success/35 bg-success/[0.04]"
            : "hover:border-border-hover"
      )}
      style={{ opacity: 0, animationDelay: "0.06s", animationFillMode: "forwards" }}
    >
      <div className="mb-2.5">
        <span className="font-mono text-[10px] font-semibold uppercase tracking-[0.2em] text-text-muted">
          Shortcut
        </span>
      </div>

      <label className="mb-2.5 block text-[13.5px] font-medium text-text-primary">
        Push-to-talk hotkey
      </label>

      {/* ── Display state ── */}
      {state === "display" && (
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2">
            {currentLabels.map((key, i) => (
              <div key={key} className="contents">
                {i > 0 && (
                  <span className="select-none text-xs text-text-muted">+</span>
                )}
                <Kbd>{key}</Kbd>
              </div>
            ))}
          </div>
          <Button size="sm" variant="ghost" onClick={handleRemap} className="ml-1 text-amber-300 hover:text-amber-200">
            Remap
          </Button>
          {!isDefault && (
            <Button size="sm" variant="ghost" onClick={handleReset} className="text-text-muted">
              Reset
            </Button>
          )}
        </div>
      )}

      {/* ── Listening state ── */}
      {state === "listening" && (
        <div className="flex flex-col gap-3">
          <div className="flex items-center gap-3">
            <div className="flex min-w-[160px] items-center justify-center gap-2 rounded-lg border border-amber-400/35 bg-amber-500/[0.06] px-4 py-2.5">
              {captured.length === 0 ? (
                <span className="animate-pulse text-sm text-amber-300/80">
                  Press your keys...
                </span>
              ) : (
                captured.map((k, i) => (
                  <div key={k.code} className="contents">
                    {i > 0 && <span className="text-xs text-text-muted">+</span>}
                    <kbd className="rounded-md border border-amber-400/30 bg-amber-500/[0.14] px-2.5 py-1 font-mono text-sm text-amber-200">
                      {k.label}
                    </kbd>
                  </div>
                ))
              )}
            </div>
            <Button size="sm" variant="ghost" onClick={handleCancel} className="text-text-muted">
              Cancel
            </Button>
          </div>
          <p className="text-xs text-text-muted">
            Press 1 or 2 keys simultaneously, then release to confirm
          </p>
        </div>
      )}

      {/* ── Confirming state ── */}
      {state === "confirming" && (
        <div className="flex flex-col gap-3">
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-2">
              {captured.map((k, i) => (
                <div key={k.code} className="contents">
                  {i > 0 && <span className="text-xs text-text-muted">+</span>}
                  <kbd className="rounded-lg border border-success/30 bg-success/[0.10] px-3 py-1.5 font-mono text-[13px] text-success shadow-sm">
                    {k.label}
                  </kbd>
                </div>
              ))}
            </div>

            <div className="ml-2 flex items-center gap-2">
              <Button size="sm" variant="primary" onClick={handleSave}>
                Save
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={handleRedo}
                icon={<RotateCcw />}
                className="text-text-muted"
                title="Try again"
              >
                Redo
              </Button>
              <Button size="sm" variant="ghost" onClick={handleCancel} className="text-text-muted">
                Cancel
              </Button>
            </div>
          </div>
        </div>
      )}
    </Card>
  );
}
