import { useQuery } from "@tanstack/react-query";
import { getAppInfo, listSites } from "./api";

/** 比较 X.Y.Z 形式的版本号：a 新返回正数，相同返回 0，a 旧返回负数。 */
export function compareVersions(a: string, b: string): number {
  const parse = (value: string) => value.replace(/^v/i, "").split(/[.+-]/).slice(0, 3).map((part) => Number.parseInt(part, 10) || 0);
  const [left, right] = [parse(a), parse(b)];
  for (let i = 0; i < 3; i += 1) {
    const diff = (left[i] ?? 0) - (right[i] ?? 0);
    if (diff !== 0) return diff;
  }
  return 0;
}

export interface MinVersionRequirement {
  /** 提出要求的站点名称。 */
  site: string;
  /** 要求的最低版本。 */
  version: string;
  /** 现在的版本。 */
  current: string;
}

/**
 * 接入的站点里，站长要求的最低版本比现在的版本高时，返回要求最高的那个；不需要更新时返回 null。
 */
export function useMinVersionRequirement(): MinVersionRequirement | null {
  const sites = useQuery({ queryKey: ["sites"], queryFn: listSites });
  const info = useQuery({ queryKey: ["app-info"], queryFn: getAppInfo });
  const current = info.data?.version;
  if (!current) return null;
  let strictest: MinVersionRequirement | null = null;
  for (const site of sites.data ?? []) {
    const version = site.config?.client?.minVersion;
    if (!version || compareVersions(current, version) >= 0) continue;
    if (!strictest || compareVersions(version, strictest.version) > 0) {
      strictest = { site: site.config?.brand.name || site.name, version, current };
    }
  }
  return strictest;
}
