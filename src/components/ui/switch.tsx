import { cn } from "../../lib/cn";

/** 开关。 */
export function Switch({
  checked,
  onChange,
  disabled,
  label,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  label: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        "relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand/60 disabled:opacity-50",
        checked ? "bg-brand" : "bg-border",
      )}
    >
      <span className={cn("inline-block h-4 w-4 rounded-full bg-white shadow-sm transition", checked ? "translate-x-[18px]" : "translate-x-0.5")} />
    </button>
  );
}
