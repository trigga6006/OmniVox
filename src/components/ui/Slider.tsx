import { forwardRef, type CSSProperties, type InputHTMLAttributes } from "react";
import { cn } from "@/lib/utils";

type Accent = "amber" | "violet" | "teal";

interface SliderProps
  extends Omit<InputHTMLAttributes<HTMLInputElement>, "type" | "value" | "onChange"> {
  value: number;
  min?: number;
  max?: number;
  step?: number;
  onChange: (value: number) => void;
  /** Structured Mode is violet, Cleanup is teal, everything else amber. */
  accent?: Accent;
}

/**
 * Range input wrapper.
 *
 * The accent travels as a `data-accent` attribute that re-points the
 * `--slider-accent` variable in globals.css, so a violet slider actually
 * renders violet — `accent-violet-500` never did, because the global
 * `::-webkit-slider-thumb` override hardcoded amber and won. The filled
 * portion of the track comes from `--slider-pct`, published here.
 */
export const Slider = forwardRef<HTMLInputElement, SliderProps>(function Slider(
  { value, min = 0, max = 100, step = 1, onChange, accent = "amber", className, style, ...props },
  ref
) {
  const span = max - min;
  const pct = span > 0 ? Math.min(100, Math.max(0, ((value - min) / span) * 100)) : 0;

  return (
    <input
      ref={ref}
      type="range"
      data-accent={accent}
      min={min}
      max={max}
      step={step}
      value={value}
      onChange={(e) => onChange(Number(e.target.value))}
      className={cn("h-1 w-full cursor-pointer", className)}
      style={{ ...style, "--slider-pct": `${pct}%` } as CSSProperties}
      {...props}
    />
  );
});
