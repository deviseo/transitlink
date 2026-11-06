import type { ToolKind } from "../../lib/api";
import { cn } from "../../lib/cn";
import antigravity from "../../assets/tools/antigravity.svg";
import claudeCode from "../../assets/tools/claude-code.svg";
import codex from "../../assets/tools/codex.svg";
import deepseek from "../../assets/tools/deepseek.svg";
import geminiCli from "../../assets/tools/gemini-cli.svg";
import grokDark from "../../assets/tools/grok-dark.svg";
import grok from "../../assets/tools/grok.svg";
import kimiDark from "../../assets/tools/kimi-dark.svg";
import kimi from "../../assets/tools/kimi.svg";
import minimax from "../../assets/tools/minimax.svg";
import ohMyPi from "../../assets/tools/oh-my-pi.svg";
import openclaw from "../../assets/tools/openclaw.svg";
import opencodeDark from "../../assets/tools/opencode-dark.svg";
import opencode from "../../assets/tools/opencode.svg";
import piDark from "../../assets/tools/pi-dark.svg";
import pi from "../../assets/tools/pi.svg";

/**
 * 各工具自己的 logo（来源和许可见 `assets/tools/LICENSES.md`）。单色的 logo 另有白色版本，
 * 系统是深色模式时用它。
 */
const LOGOS: Record<ToolKind, { light: string; dark?: string }> = {
  claudeCode: { light: claudeCode },
  codex: { light: codex },
  geminiCli: { light: geminiCli },
  grokBuild: { light: grok, dark: grokDark },
  opencode: { light: opencode, dark: opencodeDark },
  deepseekHarness: { light: deepseek },
  pi: { light: pi, dark: piDark },
  ohMyPi: { light: ohMyPi },
  kimiCode: { light: kimi, dark: kimiDark },
  minimaxCode: { light: minimax },
  openclaw: { light: openclaw },
  antigravity: { light: antigravity },
};

const SIZES = {
  sm: "h-7 w-7 rounded-lg p-[5px]",
  md: "h-10 w-10 rounded-xl p-2",
  lg: "h-14 w-14 rounded-2xl p-[11px]",
};

/** 工具图标：统一的浅色底块，中间是工具的 logo。 */
export function ToolIcon({ kind, size = "md", className }: { kind: ToolKind; size?: keyof typeof SIZES; className?: string }) {
  const logo = LOGOS[kind];
  return (
    <span
      className={cn(
        "inline-flex shrink-0 select-none items-center justify-center bg-white ring-1 ring-black/[0.07] dark:bg-white/[0.07] dark:ring-white/10",
        SIZES[size],
        className,
      )}
      aria-hidden
    >
      <picture className="flex h-full w-full">
        {logo.dark && <source srcSet={logo.dark} media="(prefers-color-scheme: dark)" />}
        <img src={logo.light} alt="" draggable={false} className="h-full w-full object-contain" />
      </picture>
    </span>
  );
}
