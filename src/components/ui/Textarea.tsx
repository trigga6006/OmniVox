import {
  forwardRef,
  useCallback,
  useLayoutEffect,
  useRef,
  type TextareaHTMLAttributes,
} from "react";
import { cn } from "@/lib/utils";
import { INPUT_CHROME } from "./Input";

interface TextareaProps extends TextareaHTMLAttributes<HTMLTextAreaElement> {
  /** Grow to fit the content instead of scrolling. */
  autosize?: boolean;
}

/**
 * Input's exact chrome in a multi-line box. Replaces the raw `<textarea>`s
 * copy-pasted across Notes / Meetings / Command test boxes.
 */
export const Textarea = forwardRef<HTMLTextAreaElement, TextareaProps>(function Textarea(
  { className, autosize, rows = 3, value, onChange, ...props },
  ref
) {
  const inner = useRef<HTMLTextAreaElement | null>(null);

  const setRefs = useCallback(
    (node: HTMLTextAreaElement | null) => {
      inner.current = node;
      if (typeof ref === "function") ref(node);
      else if (ref) ref.current = node;
    },
    [ref]
  );

  const resize = useCallback(() => {
    const el = inner.current;
    if (!el || !autosize) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [autosize]);

  // Re-measure on every value change, including programmatic ones.
  useLayoutEffect(resize, [resize, value]);

  return (
    <textarea
      ref={setRefs}
      rows={rows}
      value={value}
      onChange={(e) => {
        onChange?.(e);
        resize();
      }}
      className={cn(
        INPUT_CHROME,
        "py-2 leading-relaxed",
        autosize ? "resize-none overflow-hidden" : "resize-y",
        className
      )}
      {...props}
    />
  );
});
