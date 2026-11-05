import { useQuery } from "@tanstack/react-query";
import { detectTools, getAppSettings, listSites, type SiteView } from "./api";

/** 应用设置（当前站点、关掉窗口后留在托盘等）的缓存键。 */
export const settingsKey = ["app-settings"] as const;

/**
 * 界面上的当前站点：概览、密钥、工单都看这个站点，左上角的切换器显示它。
 * 用户选过的优先；没选过（或那个站点删了）时用 Claude Code / Codex 在用的站点，再没有就用最近接入的。
 * `loaded`：站点和设置都读完了，不会再从自动挑的站点跳到别的站点。
 */
export function useCurrentSite(): { site: SiteView | null; sites: SiteView[]; loaded: boolean } {
  const sites = useQuery({ queryKey: ["sites"], queryFn: listSites });
  const settings = useQuery({ queryKey: settingsKey, queryFn: getAppSettings });
  const tools = useQuery({ queryKey: ["tools"], queryFn: detectTools });
  const list = sites.data ?? [];
  const chosen = list.find((item) => item.id === settings.data?.currentSite);
  const inUse = new Set((tools.data ?? []).map((tool) => tool.applied?.siteId).filter(Boolean));
  const site = chosen ?? list.find((item) => inUse.has(item.id)) ?? [...list].sort((a, b) => b.addedAt - a.addedAt)[0] ?? null;
  const loaded = sites.data !== undefined && !settings.isPending && (chosen !== undefined || !tools.isPending);
  return { site, sites: list, loaded };
}

/** 站点在界面上的名字：站长的品牌名，没有时用站点自己的名字。 */
export function siteName(site: SiteView): string {
  return site.config?.brand.name || site.name;
}

/** 网址里的主机名（显示用）。 */
export function siteHost(site: SiteView): string {
  try {
    return new URL(site.baseUrl).host;
  } catch {
    return site.baseUrl;
  }
}
