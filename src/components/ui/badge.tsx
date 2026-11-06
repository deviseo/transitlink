import type { HTMLAttributes } from "react";
import { cn } from "../../lib/cn";

export type Tone = "neutral" | "success" | "danger" | "brand" | "warning";

const tones: Record<Tone, string> = {
  neutral: "bg-muted text-muted-foreground",
  success: "bg-success/10 text-success",
  danger: "bg-danger/10 text-danger",
  brand: "bg-brand/10 text-brand",
  warning: "bg-warning/10 text-warning",
};

export function Badge({ tone = "neutral", className, ...props }: HTMLAttributes<HTMLSpanElement> & { tone?: Tone }) {
  return <span className={cn("inline-flex shrink-0 items-center gap-1 whitespace-nowrap rounded-full px-2 py-0.5 text-[11px] font-medium leading-4", tones[tone], className)} {...props} />;
}

/** 状态小圆点（和文字一起用，不单靠颜色表达意思）。 */
export function Dot({ tone = "neutral", className }: { tone?: Tone; className?: string }) {
  const colors: Record<Tone, string> = {
    neutral: "bg-muted-foreground/40",
    success: "bg-success",
    danger: "bg-danger",
    brand: "bg-brand",
    warning: "bg-warning",
  };
  return <span className={cn("inline-block h-1.5 w-1.5 shrink-0 rounded-full", colors[tone], className)} aria-hidden />;
}
