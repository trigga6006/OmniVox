import { type HTMLAttributes } from "react";
import { cn } from "@/lib/utils";

type Tone = "amber" | "violet" | "blue" | "red" | "green" | "neutral";

const TONES: Record<Tone, string> = {
  amber: "text-amber-300 bg-amber-500/[0.14]",
  violet: "text-violet-300 bg-violet-500/[0.14]",
  blue: "text-indigo-300 bg-indigo-500/[0.14]",
  red: "text-recording-400 bg-recording-500/[0.14]",
  green: "text-success bg-success/[0.14]",
  neutral: "text-text-secondary bg-surface-3",
};

interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  tone?: Tone;
}

export function Badge({ tone = "neutral", className, ...props }: BadgeProps) {
  return (
    <span
      className={cn(
        "inline-flex items-center rounded-md px-1.5 py-0.5",
        "font-mono text-[10px] font-semibold uppercase tracking-[0.04em]",
        TONES[tone],
        className
      )}
      {...props}
    />
  );
}
