import { type HTMLAttributes } from "react";
import { cn } from "@/lib/utils";
import { TONE_TEXT, TONE_TINT, type Tone } from "./tones";

interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  tone?: Tone;
}

export function Badge({ tone = "neutral", className, ...props }: BadgeProps) {
  return (
    <span
      className={cn(
        "inline-flex items-center rounded-[var(--radius-s)] px-1.5 py-0.5",
        "font-mono text-2xs font-semibold uppercase tracking-[0.04em]",
        TONE_TEXT[tone],
        TONE_TINT[tone],
        className
      )}
      {...props}
    />
  );
}
