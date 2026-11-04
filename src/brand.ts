/** 当前显示的品牌。默认是 TransitLink；接入站长定制的站点后，由 TransitHub 下发的站点配置替换。 */
export interface Brand {
  name: string;
  logoUrl: string;
  color: string;
}

export const defaultBrand: Brand = {
  name: "TransitLink",
  logoUrl: "/logo.png",
  color: "",
};

/** 把品牌主色写入 CSS 变量；空值表示使用默认主题色。 */
export function applyBrandColor(color: string) {
  const root = document.documentElement;
  if (color) root.style.setProperty("--brand", color);
  else root.style.removeProperty("--brand");
}
