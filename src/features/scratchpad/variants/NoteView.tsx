import type { Ref } from "react";

/** The single continuous editable note (`note` variant). Dictation appends to
 *  the end; the user can also type/edit freely. */
export function NoteView({
  note,
  onChange,
  textareaRef,
}: {
  note: string;
  onChange: (text: string) => void;
  textareaRef?: Ref<HTMLTextAreaElement>;
}) {
  return (
    <textarea
      ref={textareaRef}
      value={note}
      onChange={(e) => onChange(e.target.value)}
      placeholder="Dictate or type anything here…"
      spellCheck={false}
      className="select-text h-full w-full resize-none bg-transparent px-3.5 py-3 text-[13px] leading-relaxed text-text-primary placeholder:text-text-muted focus:outline-none"
    />
  );
}
