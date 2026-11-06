import { cn } from "../../lib/cn";

/** 分段切换（筛选用）。 */
export function Segmented<T extends string>({
  value,
  onChange,
  options,
  label,
}: {
  value: T;
  onChange: (value: T) => void;
  options: { value: T; label: string; count?: number }[];
  label: string;
}) {
  return (
    <div role="tablist" aria-label={label} className="inline-flex rounded-lg bg-muted p-0.5">
      {options.map((option) => {
        const selected = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            role="tab"
            aria-selected={selected}
            onClick={() => onChange(option.value)}
            className={cn(
              "inline-flex h-7 items-center gap-1.5 rounded-md px-3 text-[13px] transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand/60",
              selected ? "bg-card font-medium text-foreground shadow-[0_1px_2px_rgba(16,24,40,0.08)]" : "text-muted-foreground hover:text-foreground",
            )}
          >
            {option.label}
            {option.count !== undefined && <span className="text-xs tabular-nums text-muted-foreground">{option.count}</span>}
          </button>
        );
      })}
    </div>
  );
}
