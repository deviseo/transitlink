import { cn } from "../../lib/cn";

/** 说明列表里的一行：左边标签，右边值；路径类的值用等宽字体并允许换行。 */
export function InfoRow({ label, value, mono = false }: { label: string; value: string | null | undefined; mono?: boolean }) {
  return (
    <div className="flex gap-3">
      <dt className="w-20 shrink-0 text-muted-foreground">{label}</dt>
      <dd className={cn("min-w-0 flex-1 break-all text-foreground", mono && "font-mono text-xs leading-5")}>{value || "—"}</dd>
    </div>
  );
}
