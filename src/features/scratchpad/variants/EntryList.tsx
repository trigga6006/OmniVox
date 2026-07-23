import { Copy, Trash2 } from "lucide-react";
import type { ScratchpadEntry } from "@/lib/tauri";

function formatTime(iso: string): string {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return "";
  return d.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
}

/** The card list shared by the `entries` and `pads` variants. */
export function EntryList({
  entries,
  onDelete,
  onCopy,
}: {
  entries: ScratchpadEntry[];
  onDelete: (id: string) => void;
  onCopy: (text: string) => void;
}) {
  if (entries.length === 0) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 px-6 text-center">
        <div className="text-[13px] font-medium text-text-secondary">Nothing here yet</div>
        <div className="text-[11px] leading-relaxed text-text-muted">
          Hit <span className="text-amber-300">Dictate</span> — or your global hotkey while
          this window is focused — to drop a note in.
        </div>
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-1.5 p-2.5">
      {entries.map((e) => (
        <div
          key={e.id}
          className="group rounded-xl border border-white/8 bg-white/[0.03] px-3 py-2.5 transition-colors hover:border-white/[0.14]"
        >
          <p className="select-text whitespace-pre-wrap break-words text-[12.5px] leading-relaxed text-text-primary/90">
            {e.content}
          </p>
          <div className="mt-1.5 flex items-center gap-2">
            <span className="text-[10px] tabular-nums text-text-muted/70">
              {formatTime(e.created_at)}
            </span>
            <div className="ml-auto flex items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100">
              <button
                onClick={() => onCopy(e.content)}
                aria-label="Copy"
                className="rounded-md p-1 text-text-muted transition-colors hover:bg-white/10 hover:text-text-secondary"
              >
                <Copy size={12} />
              </button>
              <button
                onClick={() => onDelete(e.id)}
                aria-label="Delete"
                className="rounded-md p-1 text-text-muted transition-colors hover:bg-recording-500/10 hover:text-recording-400"
              >
                <Trash2 size={12} />
              </button>
            </div>
          </div>
        </div>
      ))}
    </div>
  );
}
