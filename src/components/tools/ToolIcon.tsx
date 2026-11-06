import type { ToolKind } from "../../lib/api";
import { cn } from "../../lib/cn";

/** 工具的图标：不用各家的商标，用名字缩写配上各自的主色。 */
const ICONS: Record<ToolKind, { label: string; background: string }> = {
  claudeCode: { label: "CC", background: "#d97757" },
  codex: { label: "Cx", background: "#111827" },
  geminiCli: { label: "G", background: "linear-gradient(135deg, #4285f4 0%, #9b72cb 55%, #d96570 100%)" },
  grokBuild: { label: "Gk", background: "#18181b" },
  opencode: { label: "OC", background: "#52525b" },
  deepseekHarness: { label: "DS", background: "#4d6bfe" },
  pi: { label: "π", background: "#6d28d9" },
  ohMyPi: { label: "Oπ", background: "linear-gradient(135deg, #db2777 0%, #7c3aed 100%)" },
  kimiCode: { label: "K", background: "#0f172a" },
  minimaxCode: { label: "MM", background: "#e11d48" },
  openclaw: { label: "Cl", background: "#ea580c" },
  antigravity: { label: "AG", background: "linear-gradient(135deg, #1a73e8 0%, #34a853 100%)" },
};

const SIZES = {
  sm: "h-7 w-7 rounded-lg text-[11px]",
  md: "h-10 w-10 rounded-xl text-[13px]",
  lg: "h-14 w-14 rounded-2xl text-lg",
};

export function ToolIcon({ kind, size = "md", className }: { kind: ToolKind; size?: keyof typeof SIZES; className?: string }) {
  const icon = ICONS[kind];
  return (
    <span
      className={cn("inline-flex shrink-0 select-none items-center justify-center font-semibold tracking-tight text-white shadow-[inset_0_0_0_1px_rgba(255,255,255,0.12)]", SIZES[size], className)}
      style={{ background: icon.background }}
      aria-hidden
    >
      {icon.label}
    </span>
  );
}
