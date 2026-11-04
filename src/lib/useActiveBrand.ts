import { useQuery } from "@tanstack/react-query";
import { defaultBrand, type Brand } from "../brand";
import { detectTools, listSites, type SiteView } from "./api";

export interface ActiveBrand extends Brand {
  welcome: string;
  /** 品牌来自哪个站点（默认的 TransitLink 品牌时为空）。 */
  site: SiteView | null;
}

/**
 * 当前显示的品牌：工具正在用的站点的站长品牌；没有时用最近接入的、有站长配置的站点；
 * 都没有时是 TransitLink 自己的。
 */
export function useActiveBrand(): ActiveBrand {
  const sites = useQuery({ queryKey: ["sites"], queryFn: listSites });
  const tools = useQuery({ queryKey: ["tools"], queryFn: detectTools });
  const inUse = new Set((tools.data ?? []).map((tool) => tool.applied?.siteId).filter(Boolean));
  const branded = (sites.data ?? []).filter((site) => site.config);
  const site = branded.find((item) => inUse.has(item.id)) ?? [...branded].sort((a, b) => b.addedAt - a.addedAt)[0];
  if (!site?.config) return { ...defaultBrand, welcome: "", site: null };
  const { brand } = site.config;
  return { name: brand.name || site.name, logoUrl: brand.logoUrl, color: brand.color, welcome: brand.welcome, site };
}
