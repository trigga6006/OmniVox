import { useCallback, useEffect, useRef, useState } from "react";
import {
  Mic,
  Square,
  X,
  Eraser,
  Crosshair,
  Copy,
  Check,
  MoreHorizontal,
  FileText,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  scratchpadGet,
  scratchpadSetVariant,
  scratchpadSetNote,
  scratchpadAddEntry,
  scratchpadDeleteEntry,
  scratchpadClearPad,
  closeScratchpad,
  saveScratchpadPosition,
  saveScratchpadSize,
  setScratchpadCapture,
  scratchpadGetCapture,
  startRecording,
  stopRecording,
  addNote,
  onRecordingStateChange,
  onDictationInsert,
  onScratchpadRefresh,
  onAudioLevel,
  type ScratchpadData,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useWindowHotkeyBridge } from "@/hooks/useWindowHotkeyBridge";
import { EntryList } from "./variants/EntryList";
import { NoteView } from "./variants/NoteView";

// Two variants now — "pads" was dropped (it duplicated "entries").
type Variant = "entries" | "note";
const VARIANTS: { id: Variant; label: string }[] = [
  { id: "entries", label: "Cards" },
  { id: "note", label: "Note" },
];

export function Scratchpad() {
  const [variant, setVariant] = useState<Variant>("entries");
  const [data, setData] = useState<ScratchpadData | null>(null);
  const [recording, setRecording] = useState(false);
  const [loadFailed, setLoadFailed] = useState(false);
  const [capturing, setCapturing] = useState(false);
  const noteSaveTimer = useRef<number | null>(null);
  const startedByUs = useRef(false);
  const noteRef = useRef<HTMLTextAreaElement>(null);

  // Forward LCtrl+LAlt (and the other combo modifiers) into the backend hotkey
  // state machine while this window is focused — the OS keyboard hook doesn't
  // fire for our own WebViews, so without this the dictation hotkey is dead in
  // the pad and only the mic button works.  A hotkey dictation started here is
  // routed back to the pad because its foreground window is the scratchpad.
  useWindowHotkeyBridge();

  // Transparent window chrome so the card's rounded corners show the desktop
  // through them. No outer drop shadow — it would bleed into the transparent
  // window edge and composite as a gray "ghost" halo over whatever is behind.
  useEffect(() => {
    document.documentElement.style.background = "transparent";
    document.documentElement.dataset.theme = "dark";
    document.body.style.background = "transparent";
    document.body.style.margin = "0";
    document.body.style.overflow = "hidden";
  }, []);

  const refresh = useCallback(async () => {
    try {
      const d = await scratchpadGet();
      setData(d);
      // Coerce a legacy "pads" setting to Cards.
      setVariant(d.variant === "note" ? "note" : "entries");
      setLoadFailed(false);
    } catch (e) {
      console.error("scratchpad load failed:", e);
      setLoadFailed(true);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // Reload when a voice command changes the stored content externally
  // ("clear the scratchpad" from Command Mode).  A pending debounced note save
  // would rewrite the cleared note right back — kill it before reloading.
  useEffect(() => {
    const un = onScratchpadRefresh(() => {
      if (noteSaveTimer.current) {
        window.clearTimeout(noteSaveTimer.current);
        noteSaveTimer.current = null;
      }
      refresh();
    });
    return () => {
      un.then((f) => f());
    };
  }, [refresh]);

  // Read the capture flag (default on) + persist window position/size on
  // move/resize so the pad reopens where — and at the size — you left it.
  useEffect(() => {
    scratchpadGetCapture().then(setCapturing).catch(() => {});
    const win = getCurrentWindow();
    let moveTimer: number | null = null;
    let sizeTimer: number | null = null;
    const unlistenP = win.onMoved(({ payload }) => {
      if (moveTimer) window.clearTimeout(moveTimer);
      moveTimer = window.setTimeout(() => {
        saveScratchpadPosition(payload.x, payload.y).catch(() => {});
      }, 300);
    });
    const unlistenS = win.onResized(({ payload }) => {
      // Minimize can report a degenerate size on Windows — don't persist it.
      if (payload.width < 200 || payload.height < 200) return;
      if (sizeTimer) window.clearTimeout(sizeTimer);
      sizeTimer = window.setTimeout(() => {
        saveScratchpadSize(payload.width, payload.height).catch(() => {});
      }, 300);
    });
    return () => {
      if (moveTimer) window.clearTimeout(moveTimer);
      if (sizeTimer) window.clearTimeout(sizeTimer);
      unlistenP.then((f) => f());
      unlistenS.then((f) => f());
    };
  }, []);

  const toggleCapture = useCallback(() => {
    setCapturing((c) => {
      const next = !c;
      setScratchpadCapture(next).catch((e) => console.error(e));
      return next;
    });
  }, []);

  useEffect(() => {
    const un = onRecordingStateChange((s) => {
      // Only reflect recordings THIS window started — a global-hotkey dictation
      // aimed at another app must not turn the mic into a Stop that aborts it.
      if (s === "recording") {
        setRecording(startedByUs.current);
      } else {
        setRecording(false);
        if (s === "idle") startedByUs.current = false;
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  const defaultPad = data?.pads.find((p) => p.id === "default") ?? data?.pads[0] ?? null;
  const bodyEntries = defaultPad?.entries ?? [];

  // Mirror current variant + note into a ref so the (once-registered) capture
  // listener always appends to the right place.
  const appendRef = useRef({ variant, note: data?.note ?? "" });
  appendRef.current = { variant, note: data?.note ?? "" };

  useEffect(() => {
    const un = onDictationInsert(({ text, target }) => {
      // The backend tags each in-app dictation with the target window; only
      // append the ones actually aimed here.
      if (target !== "scratchpad") return;
      const t = text.trim();
      if (!t) return;
      const { variant: v, note } = appendRef.current;
      if (v === "note") {
        const sep = note.length > 0 && !/\s$/.test(note) ? " " : "";
        const next = note + sep + t;
        if (noteSaveTimer.current) {
          window.clearTimeout(noteSaveTimer.current);
          noteSaveTimer.current = null;
        }
        setData((d) => (d ? { ...d, note: next } : d));
        scratchpadSetNote(next).catch((e) => console.error(e));
        // Land the caret at the END of the inserted text (and scroll it into
        // view) so a multi-part dictation — or typing — continues from there,
        // instead of restoring the stale pre-insert position.
        requestAnimationFrame(() => {
          const el = noteRef.current;
          if (el) {
            el.setSelectionRange(next.length, next.length);
            el.scrollTop = el.scrollHeight;
          }
        });
      } else {
        scratchpadAddEntry(null, t)
          .then((entry) => {
            setData((d) =>
              d
                ? {
                    ...d,
                    pads: d.pads.map((p) =>
                      p.id === entry.pad_id ? { ...p, entries: [entry, ...p.entries] } : p
                    ),
                  }
                : d
            );
          })
          .catch((e) => console.error(e));
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  const toggleMic = useCallback(() => {
    if (recording) {
      stopRecording().catch((e) => console.error(e));
    } else {
      startedByUs.current = true;
      startRecording().catch((e) => {
        startedByUs.current = false;
        console.error(e);
      });
    }
  }, [recording]);

  const switchVariant = useCallback((v: Variant) => {
    setVariant(v);
    scratchpadSetVariant(v).catch((e) => console.error(e));
  }, []);

  const handleNoteChange = useCallback((text: string) => {
    setData((d) => (d ? { ...d, note: text } : d));
    if (noteSaveTimer.current) window.clearTimeout(noteSaveTimer.current);
    noteSaveTimer.current = window.setTimeout(() => {
      scratchpadSetNote(text).catch((e) => console.error(e));
    }, 400);
  }, []);
  useEffect(
    () => () => {
      if (noteSaveTimer.current) window.clearTimeout(noteSaveTimer.current);
    },
    []
  );

  const deleteEntry = useCallback((id: string) => {
    setData((d) =>
      d ? { ...d, pads: d.pads.map((p) => ({ ...p, entries: p.entries.filter((e) => e.id !== id) })) } : d
    );
    scratchpadDeleteEntry(id).catch((e) => console.error(e));
  }, []);

  const copyText = useCallback((text: string) => {
    navigator.clipboard?.writeText(text).catch(() => {});
  }, []);

  // Everything currently in the pad as one string: the note as-is, or all
  // cards joined oldest→newest so the text reads in the order it was dictated.
  const allText = useCallback(() => {
    if (!data) return "";
    return variant === "note"
      ? data.note
      : [...bodyEntries]
          .reverse()
          .map((e) => e.content)
          .join("\n\n");
  }, [variant, data, bodyEntries]);

  const copyAllText = useCallback(() => {
    const text = allText();
    if (text.trim()) navigator.clipboard?.writeText(text).catch(() => {});
  }, [allText]);

  const hasCopyable =
    variant === "note" ? (data?.note.trim().length ?? 0) > 0 : bodyEntries.length > 0;

  // ⋯ menu: send-to-Notes + full wipe (cards AND note — same scope as the
  // voice "clear the scratchpad" command, unlike the old cards-only eraser).
  const [menuOpen, setMenuOpen] = useState(false);
  const [confirmClear, setConfirmClear] = useState(false);
  const [sentFlash, setSentFlash] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const sentFlashTimer = useRef<number | null>(null);

  useEffect(() => {
    if (!menuOpen) {
      setConfirmClear(false);
      return;
    }
    const onDown = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setMenuOpen(false);
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setMenuOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [menuOpen]);
  useEffect(
    () => () => {
      if (sentFlashTimer.current) window.clearTimeout(sentFlashTimer.current);
    },
    []
  );

  const handleSendToNotes = useCallback(async () => {
    const text = allText();
    if (!text.trim()) return;
    setMenuOpen(false);
    const stamp = new Date().toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
    });
    try {
      await addNote(`Scratchpad — ${stamp}`, text);
      setSentFlash(true);
      if (sentFlashTimer.current) window.clearTimeout(sentFlashTimer.current);
      sentFlashTimer.current = window.setTimeout(() => setSentFlash(false), 1400);
    } catch (e) {
      console.error("send to notes failed:", e);
    }
  }, [allText]);

  const wipeAll = useCallback(() => {
    // A pending debounced note save would resurrect the wiped note — kill it.
    if (noteSaveTimer.current) {
      window.clearTimeout(noteSaveTimer.current);
      noteSaveTimer.current = null;
    }
    setData((d) =>
      d
        ? {
            ...d,
            note: "",
            pads: d.pads.map((p) => (p.id === "default" ? { ...p, entries: [] } : p)),
          }
        : d
    );
    scratchpadClearPad(null).catch((e) => console.error(e));
    scratchpadSetNote("").catch((e) => console.error(e));
  }, []);

  const handleWipe = useCallback(() => {
    if (!confirmClear) {
      setConfirmClear(true);
      return;
    }
    setConfirmClear(false);
    setMenuOpen(false);
    wipeAll();
  }, [confirmClear, wipeAll]);

  // Stop header-button clicks from being swallowed by the title drag region.
  const noDrag = (e: React.MouseEvent) => e.stopPropagation();

  return (
    <div className="flex h-screen w-screen select-none flex-col">
      <div
        className="relative flex flex-1 flex-col overflow-hidden rounded-2xl border border-white/10 text-text-primary"
        style={{
          background:
            "linear-gradient(180deg, rgba(26,26,30,0.98) 0%, rgba(15,15,18,0.99) 100%)",
        }}
      >
        <div
          className="pointer-events-none absolute inset-x-0 top-0 h-px"
          style={{
            background:
              "linear-gradient(90deg, transparent, rgba(255,255,255,0.10) 50%, transparent)",
          }}
        />

        {/* ── Header (drag region) ── */}
        <div
          data-tauri-drag-region
          className="flex h-10 shrink-0 items-center gap-2 border-b border-white/8 px-3"
        >
          <span
            data-tauri-drag-region
            className="font-display text-[13px] font-semibold tracking-[-0.01em] text-text-primary"
          >
            Scratchpad
          </span>

          <div className="ml-auto flex items-center gap-1.5">
            {/* Capture toggle — when on, your dictation hotkey routes here even
                while another window is focused (read there, dictate here). */}
            <button
              onMouseDown={noDrag}
              onClick={toggleCapture}
              aria-label="Toggle dictation capture"
              title={
                capturing
                  ? "Capturing your dictation into this pad — click to pause"
                  : "Paused — click to capture your dictation here"
              }
              className={cn(
                "flex h-6 w-6 items-center justify-center rounded-md transition-colors",
                capturing
                  ? "bg-amber-500/20 text-amber-300"
                  : "text-text-muted hover:bg-white/10 hover:text-text-secondary"
              )}
            >
              <Crosshair size={13} strokeWidth={2} />
            </button>
            <div
              onMouseDown={noDrag}
              className="flex items-center gap-0.5 rounded-lg bg-white/5 p-0.5"
            >
              {VARIANTS.map((v) => (
                <button
                  key={v.id}
                  onClick={() => switchVariant(v.id)}
                  className={cn(
                    "rounded-md px-2 py-1 text-[10px] font-medium transition-colors",
                    variant === v.id
                      ? "bg-amber-500/20 text-amber-200"
                      : "text-text-muted hover:text-text-secondary"
                  )}
                >
                  {v.label}
                </button>
              ))}
            </div>
            {/* ⋯ overflow menu — occasional / destructive actions */}
            <div className="relative" ref={menuRef} onMouseDown={noDrag}>
              <button
                onClick={() => setMenuOpen((o) => !o)}
                aria-label="More actions"
                aria-expanded={menuOpen}
                className={cn(
                  "flex h-6 w-6 items-center justify-center rounded-md transition-colors",
                  sentFlash
                    ? "text-amber-300"
                    : menuOpen
                      ? "bg-white/10 text-text-primary"
                      : "text-text-muted hover:bg-white/10 hover:text-text-secondary"
                )}
              >
                {sentFlash ? (
                  <Check size={13} strokeWidth={2.5} />
                ) : (
                  <MoreHorizontal size={13} strokeWidth={2.2} />
                )}
              </button>
              {menuOpen && (
                <div className="absolute right-0 top-7 z-20 w-48 rounded-lg border border-white/10 bg-[#232329] p-1 shadow-lg">
                  <button
                    onClick={handleSendToNotes}
                    disabled={!hasCopyable}
                    className={cn(
                      "flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-[11.5px] font-medium transition-colors",
                      hasCopyable
                        ? "text-text-secondary hover:bg-white/8 hover:text-text-primary"
                        : "cursor-default text-text-muted/50"
                    )}
                  >
                    <FileText size={12} strokeWidth={2} />
                    Send to Notes
                  </button>
                  <button
                    onClick={handleWipe}
                    className={cn(
                      "flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-[11.5px] font-medium transition-colors",
                      confirmClear
                        ? "bg-recording-500/15 text-recording-300"
                        : "text-text-secondary hover:bg-recording-500/10 hover:text-recording-400"
                    )}
                  >
                    <Eraser size={12} strokeWidth={2} />
                    {confirmClear ? "Click again to confirm" : "Clear scratchpad"}
                  </button>
                </div>
              )}
            </div>
            <button
              onMouseDown={noDrag}
              onClick={() => closeScratchpad().catch(() => {})}
              aria-label="Close"
              className="flex h-6 w-6 items-center justify-center rounded-md text-text-muted transition-colors hover:bg-white/10 hover:text-text-primary"
            >
              <X size={13} strokeWidth={2.2} />
            </button>
          </div>
        </div>

        {/* ── Body ── */}
        <div className="min-h-0 flex-1 overflow-y-auto">
          {data === null ? (
            <div className="flex h-full flex-col items-center justify-center gap-2 text-xs text-text-muted">
              {loadFailed ? (
                <>
                  <span>Couldn't load the scratchpad.</span>
                  <button
                    onClick={() => refresh()}
                    className="rounded-md bg-white/10 px-2.5 py-1 text-text-secondary transition-colors hover:bg-white/15"
                  >
                    Retry
                  </button>
                </>
              ) : (
                "Loading…"
              )}
            </div>
          ) : variant === "note" ? (
            <NoteView note={data.note} onChange={handleNoteChange} textareaRef={noteRef} />
          ) : (
            <EntryList entries={bodyEntries} onDelete={deleteEntry} onCopy={copyText} />
          )}
        </div>

        {/* ── Footer: voice-reactive mic orb ── */}
        <MicOrb
          recording={recording}
          disabled={data === null}
          onToggle={toggleMic}
          onCopyAll={hasCopyable ? copyAllText : undefined}
        />

        {/* Bottom-right resize grip — the undecorated transparent window's
            invisible edge hit-zones are unreliable on Windows, so give
            resizing an explicit handle. */}
        <div
          aria-hidden
          onMouseDown={(e) => {
            e.preventDefault();
            e.stopPropagation();
            getCurrentWindow()
              .startResizeDragging("SouthEast")
              .catch((err) => console.error(err));
          }}
          className="absolute bottom-0 right-0 z-10 flex h-3.5 w-3.5 cursor-se-resize items-end justify-end pb-[3px] pr-[3px] text-white/25 transition-colors hover:text-white/60"
        >
          <svg width="7" height="7" viewBox="0 0 7 7" fill="none">
            <path
              d="M6 2.5L2.5 6M6 5L5 6"
              stroke="currentColor"
              strokeWidth="1.2"
              strokeLinecap="round"
            />
          </svg>
        </div>
      </div>
    </div>
  );
}

/**
 * The dictation control. A compact orb whose halo breathes with your live audio
 * level while recording — no big center bar, just a small responsive light.
 */
function MicOrb({
  recording,
  disabled,
  onToggle,
  onCopyAll,
}: {
  recording: boolean;
  disabled: boolean;
  onToggle: () => void;
  onCopyAll?: () => void;
}) {
  const [level, setLevel] = useState(0);
  const [copied, setCopied] = useState(false);
  const copiedTimer = useRef<number | null>(null);
  useEffect(
    () => () => {
      if (copiedTimer.current) window.clearTimeout(copiedTimer.current);
    },
    []
  );
  const handleCopyAll = () => {
    onCopyAll?.();
    setCopied(true);
    if (copiedTimer.current) window.clearTimeout(copiedTimer.current);
    copiedTimer.current = window.setTimeout(() => setCopied(false), 1200);
  };
  useEffect(() => {
    if (!recording) {
      setLevel(0);
      return;
    }
    const un = onAudioLevel((l) => setLevel(Math.min(1, Math.max(0, l))));
    return () => {
      un.then((f) => f());
    };
  }, [recording]);

  return (
    <div className="relative flex h-9 shrink-0 items-center gap-2 border-t border-white/8 px-3">
      {/* Tiny fallback control — your dictation hotkey is the primary way in. */}
      <button
        onClick={onToggle}
        disabled={disabled}
        aria-label={recording ? "Stop dictation" : "Start dictation"}
        className={cn(
          "group relative flex h-5 w-5 items-center justify-center",
          disabled && "pointer-events-none opacity-40"
        )}
      >
        {/* voice-reactive halo — grows + brightens with your audio level */}
        {recording && (
          <span
            aria-hidden
            className="pointer-events-none absolute rounded-full"
            style={{
              inset: `-${4 + level * 10}px`,
              background: `radial-gradient(circle, rgba(239,68,68,${0.14 + level * 0.28}) 0%, transparent 68%)`,
              transition: "inset 90ms ease-out, background 90ms ease-out",
            }}
          />
        )}
        <span
          className={cn(
            "relative flex h-5 w-5 items-center justify-center rounded-full transition-colors duration-200",
            recording ? "bg-recording-500 text-white" : "bg-amber-500 text-black group-hover:bg-amber-400"
          )}
          style={
            recording
              ? { transform: `scale(${1 + level * 0.12})`, transition: "transform 90ms ease-out" }
              : undefined
          }
        >
          {recording ? (
            <Square size={9} strokeWidth={2.5} className="fill-current" />
          ) : (
            <Mic size={11} strokeWidth={2} />
          )}
        </span>
      </button>

      <span
        className={cn(
          "text-[10px] font-medium tracking-wide transition-colors",
          recording ? "text-recording-300/90" : "text-text-muted"
        )}
      >
        {recording ? "Listening…" : "Hotkey or tap to dictate"}
      </span>

      {onCopyAll && (
        <button
          onClick={handleCopyAll}
          aria-label="Copy all text"
          title="Copy everything in the pad"
          className={cn(
            "ml-auto flex h-6 w-6 items-center justify-center rounded-full transition-colors",
            copied
              ? "text-amber-300"
              : "text-text-muted hover:bg-white/10 hover:text-text-secondary"
          )}
        >
          {copied ? <Check size={12} strokeWidth={2.5} /> : <Copy size={12} strokeWidth={2} />}
        </button>
      )}
    </div>
  );
}
