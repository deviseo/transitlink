import type { TFunction } from "i18next";
import type { Tone } from "../components/ui/badge";
import type { SiteView, ToolOverview } from "./api";
import { siteName } from "./currentSite";

/** 工具和当前站点的关系，一句话说清（工具列表、概览、详情页共用）。 */
export function toolStatus(t: TFunction, tool: ToolOverview, current: SiteView | null, sites: SiteView[]): { tone: Tone; text: string } {
  const applied = tool.applied;
  // 不能连接站点的工具（Antigravity CLI）：只说装没装。
  if (!tool.configurable) {
    return tool.installed ? { tone: "neutral", text: t("tools.status.installedPlain") } : { tone: "neutral", text: t("tools.status.notInstalled") };
  }
  if (tool.state === "invalid") return { tone: "danger", text: t("tools.status.invalid") };
  if (tool.state === "configured" && applied) {
    if (!tool.installed) return { tone: "warning", text: t("tools.status.configuredMissing") };
    if (current && applied.siteId === current.id) {
      return { tone: "success", text: applied.model ? t("tools.status.connected", { model: applied.model }) : t("tools.status.connectedDefault") };
    }
    const site = sites.find((item) => item.id === applied.siteId);
    return { tone: "warning", text: t("tools.status.elsewhere", { site: site ? siteName(site) : applied.siteName }) };
  }
  if (tool.state === "changed") return { tone: "warning", text: t("tools.status.changed") };
  if (tool.state === "other") return { tone: "neutral", text: t("tools.status.other") };
  if (!tool.installed) return { tone: "neutral", text: t("tools.status.notInstalled") };
  return { tone: "brand", text: t("tools.status.installedOnly") };
}
