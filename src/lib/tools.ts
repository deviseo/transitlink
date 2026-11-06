import type { TFunction } from "i18next";
import type { TestResult, ToolKind } from "./api";

/** 支持的工具（工具页、一键配置的顺序）。 */
export const ALL_TOOLS: ToolKind[] = [
  "claudeCode",
  "codex",
  "geminiCli",
  "grokBuild",
  "opencode",
  "deepseekHarness",
  "pi",
  "ohMyPi",
  "kimiCode",
  "minimaxCode",
  "openclaw",
  "antigravity",
];

/** 能一键连接站点的工具（Antigravity CLI 不能）。 */
export const CONFIGURABLE_TOOLS: ToolKind[] = ALL_TOOLS.filter((kind) => kind !== "antigravity");

/** 在终端里启动工具的命令。 */
export const toolCommands: Record<ToolKind, string> = {
  claudeCode: "claude",
  codex: "codex",
  geminiCli: "gemini",
  grokBuild: "grok",
  opencode: "opencode",
  deepseekHarness: "dsh web",
  pi: "pi",
  ohMyPi: "omp",
  kimiCode: "kimi",
  minimaxCode: "mcode",
  openclaw: "openclaw",
  antigravity: "agy",
};

/** 一键配置时说明分组里应该有哪类模型（setup.family*）。 */
export const familyKey: Record<ToolKind, string> = {
  claudeCode: "setup.familyClaudeCode",
  codex: "setup.familyCodex",
  geminiCli: "setup.familyGeminiCli",
  grokBuild: "setup.familyGrokBuild",
  opencode: "setup.familyOpencode",
  deepseekHarness: "setup.familyDeepseekHarness",
  pi: "setup.familyOpencode",
  ohMyPi: "setup.familyOpencode",
  kimiCode: "setup.familyKimiCode",
  minimaxCode: "setup.familyMinimaxCode",
  openclaw: "setup.familyOpencode",
  antigravity: "setup.familyGeminiCli",
};

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
