import { call, inDesktop } from "./tauri";

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

export type Platform = "newapi" | "sub2api";

export interface SiteInfo {
  platform: Platform;
  baseUrl: string;
  name: string;
  logoUrl: string | null;
  captchaRequired: boolean;
  passwordLoginDisabled: boolean;
  emailOnly: boolean;
}

export interface SiteView {
  id: string;
  name: string;
  baseUrl: string;
  logoUrl: string | null;
  platform: Platform;
  account: string;
  addedAt: number;
}

export type LoginOutcome =
  | { status: "done"; site: SiteView }
  | { status: "needCode"; flowId: string; stage: "login" | "token" };

export interface AccountSummary {
  name: string;
  group: string | null;
  balanceUsd: number;
}

export interface KeySummary {
  id: string;
  name: string;
  key: string;
  masked: boolean;
  group: string | null;
  enabled: boolean;
}

export interface GroupSummary {
  id: string;
  name: string;
  description: string;
  ratio: number | null;
  platform: string | null;
}

/** Rust 命令返回的错误：code 对应 sites.errors.* 文案，message 是站点自己的说明。 */
export interface CommandError {
  code: string;
  message: string | null;
}

export function isCommandError(value: unknown): value is CommandError {
  return typeof value === "object" && value !== null && "code" in value;
}

// —— 浏览器预览用的示例数据（不会访问任何站点） ——

const previewSite: SiteView = {
  id: "preview",
  name: "示例中转站",
  baseUrl: "https://relay.example.com",
  logoUrl: null,
  platform: "newapi",
  account: "demo",
  addedAt: 0,
};
const previewSites: SiteView[] = [];
const previewKeys: KeySummary[] = [
  { id: "1", name: "TransitLink Claude Code", key: "sk-ab12**********yz89", masked: true, group: "claude", enabled: true },
  { id: "2", name: "默认", key: "sk-cd34**********wx67", masked: true, group: null, enabled: false },
];

function previewError(code: string): never {
  throw { code, message: null } satisfies CommandError;
}

export const getAppInfo = () =>
  call<AppInfo>("app_info", () => ({ name: "TransitLink", version: "0.1.0", dataDir: "~/.transitlink", platform: "browser" }));

export const detectTools = () =>
  call<ToolStatus[]>("detect_tools", () => [
    { kind: "claudeCode", installed: true, binaryPath: "/usr/local/bin/claude", version: "2.0.14", configPath: "~/.claude/settings.json", configExists: true },
    { kind: "codex", installed: false, binaryPath: null, version: null, configPath: "~/.codex/config.toml", configExists: false },
  ]);

export const detectSite = (url: string) =>
  call<SiteInfo>("detect_site", () => {
    if (!url.includes(".")) previewError("invalidUrl");
    return { platform: "newapi", baseUrl: `https://${url.replace(/^https?:\/\//, "").replace(/\/.*$/, "")}`, name: "示例中转站", logoUrl: null, captchaRequired: false, passwordLoginDisabled: false, emailOnly: false };
  }, { url });

export const loginSite = (url: string, account: string, password: string) =>
  call<LoginOutcome>("login_site", () => {
    if (password === "wrong") previewError("badCredentials");
    if (account.includes("2fa")) return { status: "needCode", flowId: "preview-flow", stage: "login" };
    const site = { ...previewSite, account };
    previewSites.splice(0, previewSites.length, site);
    return { status: "done", site };
  }, { url, account, password });

export const submitLoginCode = (flowId: string, code: string) =>
  call<LoginOutcome>("submit_login_code", () => {
    if (code !== "123456") previewError("invalidCode");
    const site = { ...previewSite, account: "demo-2fa" };
    previewSites.splice(0, previewSites.length, site);
    return { status: "done", site };
  }, { flowId, code });

export const cancelLogin = (flowId: string) => call<void>("cancel_login", () => undefined, { flowId });

export const listSites = () => call<SiteView[]>("list_sites", () => [...previewSites]);

export const removeSite = (id: string) =>
  call<void>("remove_site", () => {
    previewSites.splice(0, previewSites.length, ...previewSites.filter((site) => site.id !== id));
  }, { id });

export const siteAccount = (id: string) => call<AccountSummary>("site_account", () => ({ name: "demo", group: "default", balanceUsd: 12.34 }), { id });

export const siteKeys = (id: string) => call<KeySummary[]>("site_keys", () => [...previewKeys], { id });

/** 把某个 Key 的完整内容复制到剪贴板（在 Rust 侧完成，完整 Key 不进入网页）。 */
export const copySiteKey = (id: string, keyId: string) =>
  call<null>("copy_site_key", async () => {
    const key = previewKeys.find((item) => item.id === keyId);
    if (!key) previewError("unexpected");
    await navigator.clipboard.writeText(key.key.replace("**********", "-preview-full-key-"));
    return null;
  }, { id, keyId });

export const siteGroups = (id: string) =>
  call<GroupSummary[]>("site_groups", () => [
    { id: "default", name: "default", description: "默认分组", ratio: 1, platform: null },
    { id: "claude", name: "claude", description: "Claude 专用", ratio: 1.2, platform: null },
  ], { id });

export const createSiteKey = (id: string, name: string, group: string | null) =>
  call<KeySummary>("create_site_key", () => {
    const key: KeySummary = { id: String(previewKeys.length + 1), name, key: "sk-prev**********0000", masked: true, group, enabled: true };
    previewKeys.unshift(key);
    return { ...key };
  }, { id, name, group });

/** 是否在浏览器预览（预览时登录流程用示例数据：密码填 wrong 演示错误，账号含 2fa 演示验证码，验证码 123456）。 */
export const isPreview = !inDesktop;
