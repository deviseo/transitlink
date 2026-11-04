import type { TFunction } from "i18next";
import type { TestResult, ToolKind } from "./api";

/** 在终端里启动工具的命令。 */
export const toolCommands: Record<ToolKind, string> = {
  claudeCode: "claude",
  codex: "codex",
};

/** 官方安装命令（Claude Code 和 Codex 都推荐用官方安装脚本，不需要 Node.js）。 */
export function installCommand(kind: ToolKind, platform: string | undefined): string {
  const windows = platform === "windows";
  switch (kind) {
    case "claudeCode":
      return windows ? "irm https://claude.ai/install.ps1 | iex" : "curl -fsSL https://claude.ai/install.sh | bash";
    case "codex":
      return windows
        ? 'powershell -ExecutionPolicy ByPass -c "irm https://chatgpt.com/codex/install.ps1 | iex"'
        : "curl -fsSL https://chatgpt.com/codex/install.sh | sh";
  }
}

/** 连通测试结果的说明文字。 */
export function testText(t: TFunction, result: TestResult): string {
  const withMessage = (base: string, message: string | null) => (message ? t("errors.withMessage", { base, message }) : base);
  switch (result.status) {
    case "ok":
      return t("tools.testOk", { family: result.family });
    case "noFamilyModels":
      return t("errors.noFamilyModels");
    case "badKey":
      return withMessage(t("errors.badKey"), result.message);
    case "failed":
      return withMessage(t("errors.probeFailed"), result.message);
  }
}
