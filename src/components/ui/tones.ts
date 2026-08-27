/**
 * The single semantic colour source for the kit.
 *
 * Badge and ToastContainer both used to carry a private copy of "what colour
 * is an error / a warning / a success", and they had already drifted (toasts
 * said `text-error`, badges said `text-recording-400`). Everything that needs
 * to speak in tones composes these slots instead.
 *
 * Slots exist because a tone is used in four different ways — as text, as a
 * tinted chip fill, as a hairline, and as a pressable surface — and Tailwind
 * class strings have to be literal for the compiler to see them.
 */

export type Tone = "amber" | "violet" | "blue" | "red" | "green" | "neutral";

/** Foreground for the tone. Light-theme values are remapped in tokens.css. */
export const TONE_TEXT: Record<Tone, string> = {
  amber: "text-amber-300",
  violet: "text-violet-300",
  blue: "text-indigo-300",
  red: "text-recording-400",
  green: "text-success",
  neutral: "text-text-secondary",
};

/** Tinted chip fill — Badge, inline status pills. */
export const TONE_TINT: Record<Tone, string> = {
  amber: "bg-amber-500/[0.14]",
  violet: "bg-violet-500/[0.14]",
  blue: "bg-indigo-500/[0.14]",
  red: "bg-recording-500/[0.14]",
  green: "bg-success/[0.14]",
  neutral: "bg-surface-3",
};

/** Hairline in the tone — toasts, callouts, banners. */
export const TONE_EDGE: Record<Tone, string> = {
  amber: "border-amber-500/30",
  violet: "border-violet-500/30",
  blue: "border-indigo-500/30",
  red: "border-recording-500/30",
  green: "border-success/30",
  neutral: "border-border",
};

/** Pressable surface in the tone — toast action buttons. */
export const TONE_ACTION: Record<Tone, string> = {
  amber: "bg-amber-500/20 hover:bg-amber-500/30",
  violet: "bg-violet-500/20 hover:bg-violet-500/30",
  blue: "bg-indigo-500/20 hover:bg-indigo-500/30",
  red: "bg-recording-500/20 hover:bg-recording-500/30",
  green: "bg-success/20 hover:bg-success/30",
  neutral: "bg-surface-2 hover:bg-surface-3",
};

/** Toast levels are the same semantics wearing a different name. */
export const LEVEL_TONE: Record<"error" | "warn" | "info", Tone> = {
  error: "red",
  warn: "amber",
  info: "neutral",
};
