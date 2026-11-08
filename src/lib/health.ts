import type { TFunction } from "i18next";
import { call } from "./tauri";
import { listSites, previewReconfigured, type AppInfo, type ToolKind } from "./api";

/** 有多严重：error 是已经用不了了，warn 是快出问题了或者可能有问题。 */
export type Level = "warn" | "error";

/** 修复办法（按钮）。reapply 能直接修好（一键修复只做它），其他的打开对应的窗口或页面。 */
export type Fix =
  | { action: "reapply"; kind: ToolKind; siteId: string }
  | { action: "setup"; kind: ToolKind; siteId: string }
  | { action: "relogin"; siteId: string }
  | { action: "recharge"; siteId: string }
  | { action: "install"; kind: ToolKind };

/** 一个问题：文案是 health.<code>，params 填进文案。 */
export interface Finding {
  level: Level;
  code: string;
  params: Record<string, string>;
  fix?: Fix;
}

export interface ProxyCheck {
  name: string;
  /** 代理地址（去掉了用户名和密码）。 */
  url: string;
  reachable: boolean;
  findings: Finding[];
}

export interface SiteCheck {
  siteId: string;
  name: string;
  host: string;
  latencyMs: number | null;
  balanceUsd: number | null;
  findings: Finding[];
}

export interface ToolCheck {
  kind: ToolKind;
  installed: boolean;
  version: string | null;
  siteId: string;
  siteName: string;
  keyName: string;
  group: string | null;
  model: string | null;
  latencyMs: number | null;
  models: number | null;
  family: number | null;
  findings: Finding[];
}

export interface Checkup {
  checkedAt: number;
  /** 读到了终端里的环境变量（读不到时没查代理和环境变量）。 */
  shellChecked: boolean;
  proxy: ProxyCheck | null;
  sites: SiteCheck[];
  tools: ToolCheck[];
}

/** 余额、Key 提醒：哪个站点（哪个工具）出了什么问题。 */
export interface Alert extends Finding {
  id: string;
  siteId: string;
  siteName: string;
  kind?: ToolKind;
}

const pause = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));

/** 预览用的体检结果：一个余额不多的站点，Claude Code 正常，Codex 的 Key 被停用了。 */
function previewCheckup(): Checkup {
  const site = { siteId: "preview", siteName: "示例中转站" };
  return {
    checkedAt: Math.floor(Date.now() / 1000),
    shellChecked: true,
    proxy: { name: "HTTPS_PROXY", url: "http://127.0.0.1:7890", reachable: true, findings: [] },
    sites: [
      {
        siteId: site.siteId,
        name: site.siteName,
        host: "relay.example.com",
        latencyMs: 186,
        balanceUsd: 1.2,
        findings: [{ level: "warn", code: "balanceLow", params: { amount: "1.20", threshold: "2.00" }, fix: { action: "recharge", siteId: site.siteId } }],
      },
    ],
    tools: [
      { kind: "claudeCode", installed: true, version: "2.1.300", ...site, keyName: "TransitLink Claude Code", group: "claude", model: "claude-sonnet-5-5", latencyMs: 412, models: 38, family: 9, findings: [] },
      {
        kind: "codex",
        installed: true,
        version: "0.166.0",
        ...site,
        keyName: "TransitLink Codex",
        group: "codex",
        model: "gpt-6.1-sol",
        latencyMs: 377,
        models: previewReconfigured.has("codex") ? 24 : null,
        family: previewReconfigured.has("codex") ? 11 : null,
        findings: previewReconfigured.has("codex")
          ? []
          : [{ level: "error", code: "keyDisabled", params: { key: "TransitLink Codex" }, fix: { action: "reapply", kind: "codex", siteId: site.siteId } }],
      },
    ],
  };
}

/** 一键体检（只读，不改任何东西）。预览时 1.5 秒后给示例结果。 */
export const runCheckup = () =>
  call<Checkup>("run_checkup", async () => {
    await pause(1500);
    return previewCheckup();
  });

/** 预览：接入了站点后演示一条余额提醒。 */
async function previewAlerts(): Promise<Alert[]> {
  const sites = await listSites();
  if (sites.length === 0) return [];
  return [
    {
      id: `balance:${sites[0].id}`,
      siteId: sites[0].id,
      siteName: sites[0].config?.brand.name || sites[0].name,
      level: "warn",
      code: "balanceLow",
      params: { amount: "1.20", threshold: "2.00" },
      fix: { action: "recharge", siteId: sites[0].id },
    },
  ];
}

/** 现在的余额、Key 提醒（桌面端每 30 分钟查一次，有变化时发 alerts-changed 事件）。 */
export const getAlerts = () => call<Alert[]>("alerts", previewAlerts);

/** 马上查一次提醒。 */
export const refreshAlerts = () => call<Alert[]>("refresh_alerts", previewAlerts);

/**
 * 问题的文案。站点自己的说明（message）附在后面，没有说明时用错误代码（code）的文案；
 * 文件位置、变量所在的配置文件填进去。
 */
export function findingText(t: TFunction, finding: Finding, language: string): string {
  const params: Record<string, string> = { ...finding.params };
  if (finding.params.where === "") params.where = t("health.shellProfile");
  if (finding.params.at) params.date = new Date(Number(finding.params.at) * 1000).toLocaleDateString(language);
  const base = t(`health.${finding.code}`, { ...params, defaultValue: finding.code });
  const detail = finding.params.message ?? (finding.params.code ? t(`errors.${finding.params.code}`, { defaultValue: t("errors.unexpected") }) : undefined);
  return detail ? t("errors.withMessage", { base, message: detail }) : base;
}

/** 修复按钮上的字。 */
export function fixLabel(t: TFunction, finding: Finding): string {
  const fix = finding.fix;
  if (!fix) return "";
  if (fix.action === "reapply") {
    if (finding.code.startsWith("key")) return t("health.fix.newKey");
    if (finding.code === "modelMissing") return t("health.fix.newModel");
    return t("health.fix.reapply");
  }
  return t(`health.fix.${fix.action}`);
}

/**
 * 一键修复只换掉不能用的 Key（重新配置，每个工具一次）。配置被改过、模型不在列表里可能是用户自己的选择，
 * 要用户在那一条上点了才改。
 */
const AUTOMATIC = new Set(["keyMissing", "keyDisabled", "keyExpired", "keyExhausted", "keyRejected"]);

export function automaticFixes(checkup: Checkup): Extract<Fix, { action: "reapply" }>[] {
  const fixes: Extract<Fix, { action: "reapply" }>[] = [];
  for (const finding of checkup.tools.flatMap((tool) => tool.findings)) {
    const fix = finding.fix;
    if (fix?.action === "reapply" && AUTOMATIC.has(finding.code) && !fixes.some((known) => known.kind === fix.kind)) fixes.push(fix);
  }
  return fixes;
}

export function allFindings(checkup: Checkup): Finding[] {
  return [...(checkup.proxy?.findings ?? []), ...checkup.sites.flatMap((site) => site.findings), ...checkup.tools.flatMap((tool) => tool.findings)];
}

/** 工单正文最多 5000 字。 */
const MAX_REPORT = 5000;

/** 不含 Key 的文字报告（复制给客服，或者附在工单里）。用界面语言写。 */
export function checkupReport(t: TFunction, checkup: Checkup, info: AppInfo | undefined, language: string): string {
  const money = new Intl.NumberFormat(language, { style: "currency", currency: "USD", maximumFractionDigits: 2 });
  const mark = (finding: Finding) => `  ${finding.level === "error" ? "✗" : "!"} ${findingText(t, finding, language)}`;
  const paren = (text: string) => t("checkup.report.paren", { text });
  const colon = t("checkup.report.colon");
  const join = (parts: (string | null)[]) => parts.filter(Boolean).join(t("common.separator"));
  const lines: string[] = [
    t("checkup.report.title"),
    t("checkup.report.time", { time: new Date(checkup.checkedAt * 1000).toLocaleString(language) }),
  ];
  if (info) lines.push(`TransitLink ${info.version} · ${info.platform}`);

  lines.push("", t("checkup.sites"));
  if (checkup.sites.length === 0) lines.push(`· ${t("checkup.noSites")}`);
  for (const site of checkup.sites) {
    const facts = join([
      site.latencyMs != null ? t("checkup.latency", { ms: site.latencyMs }) : null,
      site.balanceUsd != null ? t("checkup.report.balance", { amount: money.format(site.balanceUsd) }) : null,
    ]);
    lines.push(`· ${site.name}${paren(site.host)}${facts ? `${colon}${facts}` : ""}`);
    lines.push(...site.findings.map(mark));
  }

  lines.push("", t("checkup.tools"));
  if (checkup.tools.length === 0) lines.push(`· ${t("checkup.noTools")}`);
  for (const tool of checkup.tools) {
    const name = [t(`tools.${tool.kind}`), tool.version].filter(Boolean).join(" ");
    const facts = join([
      tool.group ? t("checkup.report.group", { group: tool.group }) : null,
      tool.model ? t("checkup.report.model", { model: tool.model }) : null,
      tool.latencyMs != null ? t("checkup.latency", { ms: tool.latencyMs }) : null,
      tool.models != null ? t("checkup.report.models", { count: tool.models }) : null,
    ]);
    const state = tool.findings.length === 0 ? t("checkup.ok") : t("checkup.report.problems", { count: tool.findings.length });
    lines.push(`· ${name} → ${tool.siteName}${facts ? paren(facts) : ""}${colon}${state}`);
    lines.push(...tool.findings.map(mark));
  }

  lines.push("", t("checkup.terminal"));
  if (!checkup.shellChecked) lines.push(`· ${t("checkup.shellUnknown")}`);
  else if (!checkup.proxy) lines.push(`· ${t("checkup.noProxy")}`);
  else {
    const proxy = checkup.proxy;
    lines.push(`· ${proxy.name} = ${proxy.url || "?"}${colon}${proxy.reachable ? t("checkup.proxyUp") : t("checkup.proxyDownShort")}`);
    lines.push(...proxy.findings.map(mark));
  }

  const text = lines.join("\n");
  const chars = Array.from(text);
  return chars.length <= MAX_REPORT ? text : `${chars.slice(0, MAX_REPORT - 1).join("")}…`;
}
