import type { HTMLAttributes } from "react";
import { cn } from "../../lib/cn";

export type Tone = "neutral" | "success" | "danger" | "brand";

const tones: Record<Tone, string> = {
  neutral: "bg-muted text-muted-foreground",
  success: "bg-success/10 text-success",
  danger: "bg-danger/10 text-danger",
  brand: "bg-brand/10 text-brand",
};

export function Badge({ tone = "neutral", className, ...props }: HTMLAttributes<HTMLSpanElement> & { tone?: Tone }) {
  return <span className={cn("inline-flex shrink-0 items-center gap-1 whitespace-nowrap rounded-md px-2 py-0.5 text-xs font-medium", tones[tone], className)} {...props} />;
}
