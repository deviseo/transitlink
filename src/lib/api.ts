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

/** 工具配置的状态：没配置、用 TransitLink 配好了、配好后被别的程序改了、用户自己配了别的服务、配置文件格式有误。 */
export type ConfigState = "notConfigured" | "configured" | "changed" | "other" | "invalid";

/** 用 TransitLink 配置时记下的信息。 */
export interface AppliedView {
  siteId: string;
  siteName: string;
  keyName: string;
  group: string | null;
  model: string | null;
  appliedAt: number;
}

/** 工具页的一张卡片：检测结果加配置状态。 */
export interface ToolOverview extends ToolStatus {
  state: ConfigState;
  applied: AppliedView | null;
  currentBaseUrl: string | null;
  currentModel: string | null;
  currentProvider: string | null;
}

export interface ToolPlan {
  kind: ToolKind;
  recommendedGroup: string | null;
  /** 推荐的分组是站长指定的，或者按名称、平台认出来的。 */
  matched: boolean;
  /** 站长推荐的模型。 */
  recommendedModel: string | null;
}

export interface SetupPlan {
  groups: GroupSummary[];
  balanceUsd: number | null;
  tools: ToolPlan[];
}

export type TestResult =
  | { status: "ok"; models: number; family: number }
  | { status: "noFamilyModels"; models: number }
  | { status: "badKey"; message: string | null }
  | { status: "failed"; message: string | null };

export interface ApplyOutcome {
  keyName: string;
  keyCreated: boolean;
  group: string | null;
  model: string | null;
  test: TestResult;
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

/** 站长在 TransitHub 里设置的站点配置（用专属链接接入的站点才有）。 */
export interface SiteConfig {
  site: { name: string; baseUrl: string; platform: string };
  brand: { name: string; logoUrl: string; color: string; welcome: string };
  recommend: { claudeCode: { group: string; model: string }; codex: { group: string; model: string } };
  notice: { content: string; updatedAt: string | null };
  support: { type: string; label: string; value: string }[];
}

/** 专属链接读到的内容：配置地址和站长配置。 */
export interface SiteLink {
  configUrl: string;
  config: SiteConfig;
}

export interface SiteView {
  id: string;
  name: string;
  baseUrl: string;
  logoUrl: string | null;
  platform: Platform;
  account: string;
  addedAt: number;
  config: SiteConfig | null;
}

export type LoginOutcome =
  | { status: "done"; site: SiteView }
  | { status: "needCode"; flowId: string; stage: "login" | "token" }
  /** 在登录窗口里登录的 new-api 站点：创建长期令牌要再输一次密码。 */
  | { status: "needPassword"; flowId: string };

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

/** Rust 命令返回的错误：code 对应 errors.* 文案，message 是站点自己的说明或出错的文件。 */
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
  config: null,
};
const previewConfig: SiteConfig = {
  site: { name: "示例中转站", baseUrl: "https://relay.example.com", platform: "newapi" },
  brand: { name: "极速中转", logoUrl: "", color: "#7c3aed", welcome: "欢迎使用极速中转，遇到问题随时找客服。" },
  recommend: { claudeCode: { group: "claude", model: "" }, codex: { group: "codex", model: "gpt-6.1-sol" } },
  notice: { content: "本周六 2:00–4:00 例行维护，期间可能短暂无法使用。", updatedAt: "2026-11-04T08:00:00Z" },
  support: [
    { type: "telegram", label: "Telegram 群", value: "https://t.me/example" },
    { type: "qq", label: "QQ 群", value: "123456789" },
  ],
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

const previewTools: ToolOverview[] = [
  { kind: "claudeCode", installed: true, binaryPath: "/usr/local/bin/claude", version: "2.1.300", configPath: "~/.claude/settings.json", configExists: true, state: "notConfigured", applied: null, currentBaseUrl: null, currentModel: null, currentProvider: null },
  { kind: "codex", installed: false, binaryPath: null, version: null, configPath: "~/.codex/config.toml", configExists: false, state: "notConfigured", applied: null, currentBaseUrl: null, currentModel: null, currentProvider: null },
];
const previewModels: Record<ToolKind, string[]> = {
  claudeCode: ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5", "gpt-6.1-sol"],
  codex: ["gpt-6.1-sol", "gpt-6-astra", "gpt-5.5", "claude-opus-5-5"],
};
const previewTool = (kind: ToolKind) => previewTools.find((tool) => tool.kind === kind) as ToolOverview;
const pause = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));

/** 检测本机的工具和它们现在的配置。 */
export const detectTools = () => call<ToolOverview[]>("detect_tools", () => previewTools.map((tool) => ({ ...tool })));

export const detectSite = (url: string) =>
  call<SiteInfo>("detect_site", () => {
    if (!url.includes(".")) previewError("invalidUrl");
    // 预览：网址里带 captcha 的当作开了人机验证的站点。
    return { platform: "newapi", baseUrl: `https://${url.replace(/^https?:\/\//, "").replace(/\/.*$/, "")}`, name: "示例中转站", logoUrl: null, captchaRequired: url.includes("captcha"), passwordLoginDisabled: false, emailOnly: false };
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

/** 开了人机验证的站点：打开站点自己的登录页，等用户在里面登录完成。预览时 1.5 秒后当作登录完成。 */
export const webLogin = (url: string) =>
  call<LoginOutcome>("web_login", async () => {
    await pause(1500);
    return { status: "needPassword", flowId: "preview-web" };
  }, { url });

/** 关掉登录窗口。 */
export const cancelWebLogin = () => call<null>("cancel_web_login", () => null);

/** 提交密码（创建长期令牌前的验证）。预览时密码填 wrong 演示错误。 */
export const submitLoginPassword = (flowId: string, password: string) =>
  call<LoginOutcome>("submit_login_password", () => {
    if (password === "wrong") previewError("badCredentials");
    const site = { ...previewSite, account: "web-user" };
    previewSites.splice(0, previewSites.length, site);
    return { status: "done", site };
  }, { flowId, password });

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

/** 一键配置前的选项：站点的分组和每个工具推荐的分组。 */
export const setupPlan = (siteId: string) =>
  call<SetupPlan>("setup_plan", () => ({
    groups: [
      { id: "default", name: "default", description: "默认分组", ratio: 1, platform: null },
      { id: "claude", name: "claude", description: "Claude 专用", ratio: 1.2, platform: null },
      { id: "codex", name: "codex", description: "Codex 专用", ratio: 1, platform: null },
    ],
    balanceUsd: 12.34,
    tools: [
      { kind: "claudeCode", recommendedGroup: "claude", matched: true, recommendedModel: null },
      { kind: "codex", recommendedGroup: "codex", matched: true, recommendedModel: previewSites.find((site) => site.id === siteId)?.config?.recommend.codex.model || null },
    ],
  }), { siteId });

/** 一键配置一个工具：专用 Key → 测试 → 备份 → 写配置。预览时选错分组（Claude Code 选 codex）演示失败。 */
export const applyTool = (kind: ToolKind, siteId: string, groupId: string | null, model: string | null = null) =>
  call<ApplyOutcome>("apply_tool", async () => {
    await pause(700);
    if (kind === "claudeCode" && groupId === "codex") previewError("noFamilyModels");
    const site = previewSites.find((item) => item.id === siteId) ?? previewSite;
    const chosen = kind === "codex" ? (model ?? "gpt-6.1-sol") : model;
    const keyName = kind === "codex" ? "TransitLink Codex" : "TransitLink Claude Code";
    Object.assign(previewTool(kind), {
      state: "configured",
      configExists: true,
      applied: { siteId, siteName: site.name, keyName, group: groupId, model: chosen, appliedAt: Math.floor(Date.now() / 1000) },
      currentBaseUrl: site.baseUrl,
      currentModel: chosen,
      currentProvider: kind === "codex" ? "transitlink" : null,
    } satisfies Partial<ToolOverview>);
    const family = previewModels[kind].filter((id) => (kind === "codex" ? id.startsWith("gpt-") : id.includes("claude"))).length;
    return { keyName, keyCreated: true, group: groupId, model: chosen, test: { status: "ok", models: previewModels[kind].length, family } };
  }, { kind, siteId, groupId, model });

/** 用工具配置里现在的地址和 Key 测试连通。 */
export const testTool = (kind: ToolKind) =>
  call<TestResult>("test_tool", async () => {
    await pause(500);
    if (previewTool(kind).state !== "configured") previewError("notConfigured");
    return { status: "ok", models: 4, family: 3 };
  }, { kind });

/** 工具可选的模型（适合这个工具的排在前面）。 */
export const toolModels = (kind: ToolKind) => call<string[]>("tool_models", () => [...previewModels[kind]], { kind });

/** 换模型；Claude Code 传 null 表示用它自己的默认模型。 */
export const setToolModel = (kind: ToolKind, model: string | null) =>
  call<null>("set_tool_model", () => {
    const tool = previewTool(kind);
    tool.currentModel = model;
    if (tool.applied) tool.applied = { ...tool.applied, model };
    return null;
  }, { kind, model });

/** 还原到第一次用 TransitLink 配置之前。 */
export const restoreTool = (kind: ToolKind) =>
  call<null>("restore_tool", () => {
    Object.assign(previewTool(kind), { state: "notConfigured", applied: null, currentBaseUrl: null, currentModel: null, currentProvider: null } satisfies Partial<ToolOverview>);
    return null;
  }, { kind });

export interface UpdateInfo {
  version: string;
  notes: string | null;
}

/** 检查有没有新版本（GitHub Releases 上最新的正式版）。预览时总是最新。 */
export const checkUpdate = () => call<UpdateInfo | null>("check_update", () => null);

/** 下载并安装新版本，装好后应用会自动重启。 */
export const installUpdate = () => call<null>("install_update", () => null);

export interface AppSettings {
  closeToTray: boolean;
  language: string;
}

/** 托盘切换站点的结果（Rust 发来的 tray-switched 事件）。 */
export interface SwitchReport {
  siteId: string;
  switched: ToolKind[];
  failures: { kind: ToolKind; error: CommandError }[];
}

const previewSettings: AppSettings = { closeToTray: true, language: "zh-CN" };

export const getAppSettings = () => call<AppSettings>("app_settings", () => ({ ...previewSettings }));

/** 关掉窗口后是否留在托盘里运行。 */
export const setCloseToTray = (enabled: boolean) =>
  call<AppSettings>("set_close_to_tray", () => {
    previewSettings.closeToTray = enabled;
    return { ...previewSettings };
  }, { enabled });

/** 告诉桌面端界面语言，托盘菜单用同一种语言。 */
export const setLanguage = (language: string) => call<null>("set_language", () => null, { language });

/** 读取专属链接（或配置地址）指向的站长配置。预览时任何以 transitlink:// 开头的链接都返回示例配置。 */
export const loadSiteLink = (link: string) =>
  call<SiteLink>("load_site_link", async () => {
    await pause(500);
    if (!link.startsWith("transitlink://") && !link.includes("/api/client/v1/sites/")) previewError("linkInvalid");
    return { configUrl: "https://hub.example.com/api/client/v1/sites/abcdefghij", config: previewConfig };
  }, { link });

/** 用专属链接接入后，记下站长配置。 */
export const attachSiteLink = (id: string, configUrl: string) =>
  call<SiteView>("attach_site_link", () => {
    const site = previewSites.find((item) => item.id === id) ?? previewSite;
    site.config = previewConfig;
    return { ...site };
  }, { id, configUrl });

/** 更新站长配置（超过 6 小时没更新的才去读）。返回有没有更新。 */
export const refreshSiteLinks = () => call<boolean>("refresh_site_links", () => false);

/** 取走等着处理的专属链接（应用被链接打开，或运行中点了链接）。 */
export const takePendingLink = () => call<string | null>("take_pending_link", () => null);

/** 是否在浏览器预览（预览时登录流程用示例数据：密码填 wrong 演示错误，账号含 2fa 演示验证码，验证码 123456）。 */
export const isPreview = !inDesktop;
