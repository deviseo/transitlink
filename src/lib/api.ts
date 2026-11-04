import { call } from "./tauri";

export type ToolKind = "claudeCode" | "codex";

export interface ToolStatus {
  kind: ToolKind;
  installed: boolean;
  binaryPath: string | null;
  version: string | null;
  configPath: string;
  configExists: boolean;
}

export interface AppInfo {
  name: string;
  version: string;
  dataDir: string;
  platform: string;
}

export const getAppInfo = () =>
  call<AppInfo>("app_info", () => ({ name: "TransitLink", version: "0.1.0", dataDir: "~/.transitlink", platform: "browser" }));

export const detectTools = () =>
  call<ToolStatus[]>("detect_tools", () => [
    { kind: "claudeCode", installed: true, binaryPath: "/usr/local/bin/claude", version: "2.0.14", configPath: "~/.claude/settings.json", configExists: true },
    { kind: "codex", installed: false, binaryPath: null, version: null, configPath: "~/.codex/config.toml", configExists: false },
  ]);
