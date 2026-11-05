import { call, inDesktop } from "./tauri";

export type ToolKind = "claudeCode" | "codex" | "geminiCli" | "grokBuild" | "opencode";

export interface ToolStatus {
  kind: ToolKind;
  installed: boolean;
  binaryPath: string | null;
  version: string | null;
  configPath: string;
  configExists: boolean;
  /** 官方安装命令（一键安装旁边显示，也可以复制到终端里自己运行）。 */
  installCommand: string;
  /** 用 npm 安装（需要 Node.js，可以换国内镜像）。 */
  installNpm: boolean;
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
  /** 对客户端本身的要求：minVersion 是最低版本，空表示不限制（旧版本保存的配置里没有这一项）。 */
  client?: { minVersion: string };
  /** 站长有没有开工单（旧版本保存的配置里没有这一项，当作没开）。 */
  tickets?: { enabled: boolean };
}

/** 专属链接读到的内容：配置地址和站长配置。 */
export interface SiteLink {
  configUrl: string;
  config: SiteConfig;
  /** 专属链接的地址（不带协议）。 */
  linkHost: string;
  /** 专属链接和站点是同一个域名（同一个站长）。不是的话，接入窗口里提醒用户确认链接是站长发的。 */
  sameOwner: boolean;
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
  /** 专属链接里站长的 TransitHub 地址（不带协议），请用户同意用工单前给他看。 */
  ticketsHost: string | null;
  /** 用户已经同意把这个站点的登录交给这个 TransitHub 核实身份；同意前不发任何工单请求。 */
  ticketsAllowed: boolean;
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
  ticketsHost: null,
  ticketsAllowed: false,
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
  tickets: { enabled: true },
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

const notConfigured = { state: "notConfigured", applied: null, currentBaseUrl: null, currentModel: null, currentProvider: null } as const;
const missing = { installed: false, binaryPath: null, version: null, configExists: false } as const;
const previewTools: ToolOverview[] = [
  { kind: "claudeCode", installed: true, binaryPath: "/usr/local/bin/claude", version: "2.1.300", configPath: "~/.claude/settings.json", configExists: true, installCommand: "curl -fsSL https://claude.ai/install.sh | bash", installNpm: false, ...notConfigured },
  { kind: "codex", ...missing, configPath: "~/.codex/config.toml", installCommand: "curl -fsSL https://chatgpt.com/codex/install.sh | sh", installNpm: false, ...notConfigured },
  { kind: "geminiCli", ...missing, configPath: "~/.gemini/.env", installCommand: "npm install -g @google/gemini-cli", installNpm: true, ...notConfigured },
  { kind: "grokBuild", ...missing, configPath: "~/.grok/config.toml", installCommand: "curl -fsSL https://x.ai/cli/install.sh | bash", installNpm: false, ...notConfigured },
  { kind: "opencode", ...missing, configPath: "~/.config/opencode/opencode.json", installCommand: "curl -fsSL https://opencode.ai/install | bash", installNpm: false, ...notConfigured },
];
const previewModels: Record<ToolKind, string[]> = {
  claudeCode: ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5", "gpt-6.1-sol"],
  codex: ["gpt-6.1-sol", "gpt-6-astra", "gpt-5.5", "claude-opus-5-5"],
  geminiCli: ["gemini-3.1-pro-preview", "gemini-3.8-flash"],
  grokBuild: ["grok-4.7", "grok-code-fast-1"],
  opencode: ["claude-sonnet-5-5", "claude-opus-5-5", "claude-haiku-5-5"],
};
const previewToolNames: Record<ToolKind, string> = { claudeCode: "Claude Code", codex: "Codex", geminiCli: "Gemini CLI", grokBuild: "Grok Build", opencode: "OpenCode" };
const previewTool = (kind: ToolKind) => previewTools.find((tool) => tool.kind === kind) as ToolOverview;
const pause = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));

/** 检测本机的工具和它们现在的配置。 */
export const detectTools = () => call<ToolOverview[]>("detect_tools", () => previewTools.map((tool) => ({ ...tool })));

/**
 * 一键安装的结果。失败时带上安装命令，可以让用户复制到终端里自己运行。
 * pathHint：装好了，但终端里还找不到这个命令（要把这个目录加到 PATH）。
 */
export type InstallOutcome =
  | { status: "installed"; version: string | null; binaryPath: string | null; pathHint: string | null }
  | { status: "needsNode"; required: number; found: string | null }
  | { status: "npmPermission"; command: string }
  | { status: "network"; command: string }
  | { status: "timedOut"; command: string }
  | { status: "failed"; code: number | null; command: string };

/** 安装过程中的一行输出（Rust 发来的 tool-install 事件）。 */
export interface InstallLine {
  kind: ToolKind;
  line: string;
}

/** 一键安装工具（官方安装脚本或 npm）。mirror：npm 用国内镜像。预览时 2 秒后当作装好了。 */
export const installTool = (kind: ToolKind, mirror: boolean) =>
  call<InstallOutcome>("install_tool", async () => {
    await pause(2000);
    Object.assign(previewTool(kind), { installed: true, version: "1.0.0", binaryPath: `~/.local/bin/${kind}` } satisfies Partial<ToolOverview>);
    return { status: "installed", version: "1.0.0", binaryPath: `~/.local/bin/${kind}`, pathHint: "~/.local/bin" };
  }, { kind, mirror });

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

// 预览数据每次都返回新对象，和桌面端一样（否则 React Query 认为数据没变，不会刷新界面）。
export const listSites = () => call<SiteView[]>("list_sites", () => previewSites.map((site) => ({ ...site })));

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
      { id: "gemini", name: "gemini", description: "Gemini 专用", ratio: 1, platform: null },
    ],
    balanceUsd: 12.34,
    tools: [
      { kind: "claudeCode", recommendedGroup: "claude", matched: true, recommendedModel: null },
      { kind: "codex", recommendedGroup: "codex", matched: true, recommendedModel: previewSites.find((site) => site.id === siteId)?.config?.recommend.codex.model || null },
      { kind: "geminiCli", recommendedGroup: "gemini", matched: true, recommendedModel: null },
      { kind: "grokBuild", recommendedGroup: "default", matched: false, recommendedModel: null },
      { kind: "opencode", recommendedGroup: "claude", matched: true, recommendedModel: null },
    ],
  }), { siteId });

/** 一键配置一个工具：专用 Key → 测试 → 备份 → 写配置。预览时选错分组（Claude Code 选 codex）演示失败。 */
export const applyTool = (kind: ToolKind, siteId: string, groupId: string | null, model: string | null = null) =>
  call<ApplyOutcome>("apply_tool", async () => {
    await pause(700);
    if (kind === "claudeCode" && groupId === "codex") previewError("noFamilyModels");
    const site = previewSites.find((item) => item.id === siteId) ?? previewSite;
    // Claude Code 不指定模型时用它自己的默认模型；其他工具挑第一个合适的。
    const chosen = kind === "claudeCode" ? model : (model ?? previewModels[kind][0]);
    const keyName = `TransitLink ${previewToolNames[kind]}`;
    Object.assign(previewTool(kind), {
      state: "configured",
      configExists: true,
      applied: { siteId, siteName: site.name, keyName, group: groupId, model: chosen, appliedAt: Math.floor(Date.now() / 1000) },
      currentBaseUrl: site.baseUrl,
      currentModel: chosen,
      currentProvider: kind === "claudeCode" || kind === "geminiCli" ? null : "transitlink",
    } satisfies Partial<ToolOverview>);
    const family = previewModels[kind].length;
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
  /** 界面上的当前站点（没选过时没有，界面自己挑一个，见 useCurrentSite）。 */
  currentSite?: string | null;
}

/** 切换站点的结果：应用里切换（switchTools）和托盘切换（tray-switched 事件）都用它。 */
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

/** 记下界面上的当前站点（null：不指定）。只是换个站点看，不动 Claude Code / Codex 的配置。 */
export const setCurrentSite = (siteId: string | null) =>
  call<AppSettings>("set_current_site", () => {
    previewSettings.currentSite = siteId;
    return { ...previewSettings };
  }, { siteId });

/** 把这些工具切到这个站点（沿用在那个站点上选过的分组和模型）。 */
export const switchTools = (siteId: string, kinds: ToolKind[]) =>
  call<SwitchReport>("switch_tools", async () => {
    await pause(600);
    const site = previewSites.find((item) => item.id === siteId) ?? previewSite;
    for (const kind of kinds) {
      const tool = previewTool(kind);
      if (tool.applied) tool.applied = { ...tool.applied, siteId, siteName: site.name };
    }
    return { siteId, switched: kinds, failures: [] };
  }, { siteId, kinds });

/** 读取专属链接（或配置地址）指向的站长配置。预览时任何以 transitlink:// 开头的链接都返回示例配置。 */
export const loadSiteLink = (link: string) =>
  call<SiteLink>("load_site_link", async () => {
    await pause(500);
    if (!link.startsWith("transitlink://") && !link.includes("/api/client/v1/sites/")) previewError("linkInvalid");
    // 预览：链接里带 other 的当作和站点不是同一个域名的链接。
    const host = link.includes("other") ? "hub.other-example.net" : "hub.example.com";
    return { configUrl: `https://${host}/api/client/v1/sites/abcdefghij`, config: previewConfig, linkHost: host, sameOwner: !link.includes("other") };
  }, { link });

/** 用专属链接接入后，记下站长配置。 */
export const attachSiteLink = (id: string, configUrl: string) =>
  call<SiteView>("attach_site_link", () => {
    const index = previewSites.findIndex((item) => item.id === id);
    // 接受了专属链接就算确认了里面的工单服务。
    const site = { ...(previewSites[index] ?? previewSite), config: previewConfig, ticketsHost: new URL(configUrl).host, ticketsAllowed: true };
    if (index >= 0) previewSites[index] = site;
    return { ...site };
  }, { id, configUrl });

/** 更新站长配置（超过 6 小时没更新的才去读）。返回有没有更新。 */
export const refreshSiteLinks = () => call<boolean>("refresh_site_links", () => false);

/** 取走等着处理的专属链接（应用被链接打开，或运行中点了链接）。 */
export const takePendingLink = () => call<string | null>("take_pending_link", () => null);

// —— 工单（用专属链接接入、站长开了工单的站点才有） ——

/** 工单状态：open 等客服回复；pending 追问后等客服回复；replied 客服回复了；closed 已关闭。 */
export type TicketStatus = "open" | "pending" | "replied" | "closed";

/** 工单会话：站点上的用户、问题分类和图片限制（会话令牌留在桌面端，不进网页）。 */
export interface TicketSession {
  user: { id: string; name: string; email: string };
  categories: string[];
  limits: { enabled: boolean; maxImages: number; maxImageBytes: number };
}

export interface TicketSummary {
  id: string;
  title: string;
  category: string;
  status: TicketStatus;
  contact: string;
  /** 客服回复了、还没看。 */
  unread: boolean;
  lastMessageAt: string;
  createdAt: string;
}

export interface TicketList {
  items: TicketSummary[];
  /** 有新回复的工单数。 */
  unread: number;
}

export interface TicketAttachment {
  id: string;
  originalName: string;
  contentType: string;
  sizeBytes: number;
  createdAt: string;
}

export interface TicketMessage {
  id: string;
  /** customer 是用户自己，admin 是客服。 */
  authorType: "customer" | "admin";
  authorName: string;
  body: string;
  createdAt: string;
  attachments: TicketAttachment[];
}

export interface TicketDetail extends TicketSummary {
  messages: TicketMessage[];
}

/** 新建工单时填的内容。 */
export interface TicketDraft {
  title: string;
  category: string;
  body: string;
  contact: string;
}

/** 要上传的图片：文件名和 base64 内容。 */
export interface ImageUpload {
  name: string;
  dataBase64: string;
}

const minutesAgo = (minutes: number) => new Date(Date.now() - minutes * 60_000).toISOString();

/** 预览用的截图：SVG 画的终端窗口，最后一行用 accent 颜色。 */
function previewScreenshot(lines: string[], accent: string): string {
  const escape = (text: string) => text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const rows = lines
    .map((line, index) => `<text x="28" y="${76 + index * 28}" fill="${index === lines.length - 1 ? accent : "#d4d4d8"}">${escape(line)}</text>`)
    .join("");
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="680" height="${100 + lines.length * 28}" font-family="Menlo, Consolas, monospace" font-size="16"><rect width="100%" height="100%" rx="14" fill="#18181b"/><circle cx="28" cy="26" r="7" fill="#f87171"/><circle cx="50" cy="26" r="7" fill="#fbbf24"/><circle cx="72" cy="26" r="7" fill="#4ade80"/>${rows}</svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

const previewTicketImages = new Map<string, string>([
  ["att-401", previewScreenshot(["$ claude", "> 帮我看看这个函数为什么报错", "", "API Error: 401", "该令牌无权访问模型 claude-opus-5-5"], "#f87171")],
  ["att-topup", previewScreenshot(["订单号   20261103-8842", "金额     ¥100.00", "支付状态 支付成功", "", "账户余额 $0.00（未到账）"], "#fbbf24")],
]);

const previewTicketSession: TicketSession = {
  user: { id: "1", name: "demo", email: "demo@example.com" },
  categories: ["通用问题", "余额/计费", "接口调用", "账号/登录"],
  limits: { enabled: true, maxImages: 5, maxImageBytes: 5 * 1024 * 1024 },
};

const previewTickets: TicketDetail[] = [
  {
    id: "t-401",
    title: "Claude Code 提示 401，用不了",
    category: "接口调用",
    status: "replied",
    contact: "",
    unread: true,
    lastMessageAt: minutesAgo(12),
    createdAt: minutesAgo(95),
    messages: [
      {
        id: "m-401-1",
        authorType: "customer",
        authorName: "demo",
        body: "今天早上开始，Claude Code 一发消息就报 401。Key 是 TransitLink 一键配置的，昨天还好好的，截图在下面。",
        createdAt: minutesAgo(95),
        attachments: [{ id: "att-401", originalName: "报错截图.png", contentType: "image/png", sizeBytes: 182_000, createdAt: minutesAgo(95) }],
      },
      {
        id: "m-401-2",
        authorType: "admin",
        authorName: "",
        body: "您好，看截图是 Key 所在的 default 分组没有 Claude 模型的权限。我们昨晚调整了分组，Claude 模型都移到了 claude 分组。\n\n在 TransitLink 的「工具」页点「重新配置」，分组选 claude 就可以了，原来的配置会自动备份。",
        createdAt: minutesAgo(12),
        attachments: [],
      },
    ],
  },
  {
    id: "t-topup",
    title: "充值后余额没到账",
    category: "余额/计费",
    status: "open",
    contact: "QQ 123456",
    unread: false,
    lastMessageAt: minutesAgo(60 * 26),
    createdAt: minutesAgo(60 * 26),
    messages: [
      {
        id: "m-topup-1",
        authorType: "customer",
        authorName: "demo",
        body: "昨晚 10 点左右充值了 100 元，支付成功了，但余额一直是 0。订单截图附上。",
        createdAt: minutesAgo(60 * 26),
        attachments: [{ id: "att-topup", originalName: "订单.png", contentType: "image/png", sizeBytes: 96_000, createdAt: minutesAgo(60 * 26) }],
      },
    ],
  },
  {
    id: "t-timeout",
    title: "晚上高峰期 Codex 偶尔超时",
    category: "接口调用",
    status: "pending",
    contact: "",
    unread: false,
    lastMessageAt: minutesAgo(60 * 50),
    createdAt: minutesAgo(60 * 75),
    messages: [
      { id: "m-timeout-1", authorType: "customer", authorName: "demo", body: "晚上 9 点到 11 点，Codex 经常卡一两分钟才返回，有时候直接超时。", createdAt: minutesAgo(60 * 75), attachments: [] },
      { id: "m-timeout-2", authorType: "admin", authorName: "", body: "您好，晚高峰上游比较拥堵，我们已经加了一条线路。现在还会这样吗？", createdAt: minutesAgo(60 * 60), attachments: [] },
      { id: "m-timeout-3", authorType: "customer", authorName: "demo", body: "好一些了，但还是偶尔会超时，大概十次有一次。", createdAt: minutesAgo(60 * 50), attachments: [] },
    ],
  },
  {
    id: "t-invoice",
    title: "可以开发票吗",
    category: "余额/计费",
    status: "closed",
    contact: "billing@example.com",
    unread: false,
    lastMessageAt: minutesAgo(60 * 24 * 9),
    createdAt: minutesAgo(60 * 24 * 10),
    messages: [
      { id: "m-invoice-1", authorType: "customer", authorName: "demo", body: "公司报销需要发票，可以开吗？", createdAt: minutesAgo(60 * 24 * 10), attachments: [] },
      { id: "m-invoice-2", authorType: "admin", authorName: "", body: "可以的，把抬头和税号发到 billing@example.com，3 个工作日内开好。", createdAt: minutesAgo(60 * 24 * 9), attachments: [] },
    ],
  },
];


function previewSummary(ticket: TicketDetail): TicketSummary {
  const { id, title, category, status, contact, unread, lastMessageAt, createdAt } = ticket;
  return { id, title, category, status, contact, unread, lastMessageAt, createdAt };
}

function previewTicket(ticketId: string): TicketDetail {
  const ticket = previewTickets.find((item) => item.id === ticketId);
  if (!ticket) previewError("ticketsNotFound");
  return ticket;
}

/** 预览时把选的图片记下来，按 base64 开头认出格式。 */
function previewAttachments(images: ImageUpload[]): TicketAttachment[] {
  return images.map((image, index) => {
    const id = `att-${Date.now()}-${index}`;
    const type = image.dataBase64.startsWith("/9j/") ? "image/jpeg" : image.dataBase64.startsWith("R0lGOD") ? "image/gif" : image.dataBase64.startsWith("UklGR") ? "image/webp" : "image/png";
    previewTicketImages.set(id, `data:${type};base64,${image.dataBase64}`);
    return { id, originalName: image.name, contentType: type, sizeBytes: Math.floor((image.dataBase64.length * 3) / 4), createdAt: new Date().toISOString() };
  });
}

/** 同意把这个站点的登录交给专属链接里的 TransitHub 核实身份（用工单前问一次，链接换了地址要重新同意）。 */
export const allowTickets = (siteId: string) =>
  call<SiteView>("ticket_allow", () => {
    const site = previewSites.find((item) => item.id === siteId);
    if (!site) previewError("sessionExpired");
    site.ticketsAllowed = true;
    return { ...site };
  }, { siteId });

/** 工单会话：问题分类、图片限制和站点上的用户。第一次调用时桌面端用站点的登录换一个工单会话。 */
export const ticketSession = (siteId: string) => call<TicketSession>("ticket_session", () => structuredClone(previewTicketSession), { siteId });

/** 我在这个站点提过的工单（最近更新的在前），和有新回复的工单数。 */
export const listTickets = (siteId: string) =>
  call<TicketList>("tickets_list", async () => {
    await pause(300);
    const items = [...previewTickets].sort((a, b) => b.lastMessageAt.localeCompare(a.lastMessageAt)).map(previewSummary);
    return { items, unread: items.filter((item) => item.unread).length };
  }, { siteId });

/** 工单详情；打开后客服的回复就算看过了。 */
export const ticketDetail = (siteId: string, ticketId: string) =>
  call<TicketDetail>("ticket_detail", async () => {
    await pause(300);
    const ticket = previewTicket(ticketId);
    ticket.unread = false;
    return structuredClone(ticket);
  }, { siteId, ticketId });

/** 新建工单。图片最多 5 张、每张 5 MB。 */
export const createTicket = (siteId: string, draft: TicketDraft, images: ImageUpload[]) =>
  call<TicketDetail>("ticket_create", async () => {
    await pause(600);
    if (!draft.title.trim() || !draft.body.trim()) previewError("ticketsInvalid");
    const now = new Date().toISOString();
    const ticket: TicketDetail = {
      id: `t-${Date.now()}`,
      title: draft.title.trim(),
      category: draft.category || previewTicketSession.categories[0],
      status: "open",
      contact: draft.contact.trim(),
      unread: false,
      lastMessageAt: now,
      createdAt: now,
      messages: [{ id: `m-${Date.now()}`, authorType: "customer", authorName: "demo", body: draft.body.trim(), createdAt: now, attachments: previewAttachments(images) }],
    };
    previewTickets.unshift(ticket);
    return structuredClone(ticket);
  }, { siteId, draft, images });

/** 追问（可以只发图片）。工单关闭后不能再回复。 */
export const replyTicket = (siteId: string, ticketId: string, body: string, images: ImageUpload[]) =>
  call<TicketDetail>("ticket_reply", async () => {
    await pause(500);
    const ticket = previewTicket(ticketId);
    if (ticket.status === "closed") previewError("ticketsClosed");
    const now = new Date().toISOString();
    ticket.messages.push({ id: `m-${Date.now()}`, authorType: "customer", authorName: "demo", body: body.trim(), createdAt: now, attachments: previewAttachments(images) });
    ticket.lastMessageAt = now;
    if (ticket.status === "replied") ticket.status = "pending";
    return structuredClone(ticket);
  }, { siteId, ticketId, body, images });

/** 关闭工单（问题解决了）。 */
export const closeTicket = (siteId: string, ticketId: string) =>
  call<TicketDetail>("ticket_close", async () => {
    await pause(300);
    const ticket = previewTicket(ticketId);
    ticket.status = "closed";
    return structuredClone(ticket);
  }, { siteId, ticketId });

/** 消息里的一张图片（`data:` 地址）。 */
export const ticketAttachment = (siteId: string, attachmentId: string, thumb: boolean) =>
  call<string>("ticket_attachment", async () => {
    await pause(thumb ? 200 : 900);
    const image = previewTicketImages.get(attachmentId);
    if (!image) previewError("ticketsNotFound");
    return image;
  }, { siteId, attachmentId, thumb });

/** 是否在浏览器预览（预览时登录流程用示例数据：密码填 wrong 演示错误，账号含 2fa 演示验证码，验证码 123456）。 */
export const isPreview = !inDesktop;

/**
 * 浏览器预览：网址带 `?demo`（例如 `http://localhost:1420/?demo#/`）时预先放两个站点，方便看多站点的界面：
 * 一个用专属链接接入、开了工单、Claude Code 在用；一个用网址接入。桌面端不受影响。
 */
if (!inDesktop && new URLSearchParams(window.location.search).has("demo")) {
  previewSites.push(
    { ...previewSite, account: "demo@example.com", addedAt: 2, config: previewConfig, ticketsHost: "hub.example.com", ticketsAllowed: true },
    { id: "preview-2", name: "另一个中转站", baseUrl: "https://api.example.org", logoUrl: null, platform: "sub2api", account: "me@example.org", addedAt: 1, config: null, ticketsHost: null, ticketsAllowed: false },
  );
  previewSettings.currentSite = "preview";
  Object.assign(previewTool("claudeCode"), {
    state: "configured",
    applied: { siteId: "preview", siteName: previewSite.name, keyName: "TransitLink Claude Code", group: "claude", model: "claude-sonnet-5-5", appliedAt: 0 },
  } satisfies Partial<ToolOverview>);
}
