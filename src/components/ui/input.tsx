import type { InputHTMLAttributes, ReactNode, TextareaHTMLAttributes } from "react";
import { cn } from "../../lib/cn";

export function Input({ className, ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      className={cn(
        "h-9 w-full rounded-lg border border-border bg-card px-3 text-sm text-foreground outline-none placeholder:text-muted-foreground focus:border-brand focus:ring-1 focus:ring-brand disabled:opacity-60",
        className,
      )}
      {...props}
    />
  );
}

export function Textarea({ className, ...props }: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return (
    <textarea
      className={cn(
        "min-h-24 w-full resize-y rounded-lg border border-border bg-card px-3 py-2 text-sm leading-6 text-foreground outline-none placeholder:text-muted-foreground focus:border-brand focus:ring-1 focus:ring-brand disabled:opacity-60",
        className,
      )}
      {...props}
    />
  );
}

/** 表单的一项：标签、输入框和说明。`aside` 显示在标签右边（例如字数）。 */
export function Field({ label, htmlFor, children, hint, aside }: { label: string; htmlFor: string; children: ReactNode; hint?: string; aside?: ReactNode }) {
  const labelNode = (
    <label htmlFor={htmlFor} className="text-sm font-medium text-foreground">
      {label}
    </label>
  );
  return (
    <div className="space-y-1.5">
      {aside ? (
        <div className="flex items-baseline justify-between gap-3">
          {labelNode}
          {aside}
        </div>
      ) : (
        labelNode
      )}
      {children}
      {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
    </div>
  );
}
