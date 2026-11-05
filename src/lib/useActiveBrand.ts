import { defaultBrand, type Brand } from "../brand";
import type { SiteView } from "./api";
import { useCurrentSite } from "./currentSite";

export interface ActiveBrand extends Brand {
  welcome: string;
  /** 品牌来自哪个站点（默认的 TransitLink 品牌时为空）。 */
  site: SiteView | null;
}

/** 当前显示的品牌：当前站点是用专属链接接入的，就用站长的品牌；否则是 TransitLink 自己的。 */
export function useActiveBrand(): ActiveBrand {
  const { site } = useCurrentSite();
  if (!site?.config) return { ...defaultBrand, welcome: "", site: null };
  const { brand } = site.config;
  return { name: brand.name || site.name, logoUrl: brand.logoUrl, color: brand.color, welcome: brand.welcome, site };
}
