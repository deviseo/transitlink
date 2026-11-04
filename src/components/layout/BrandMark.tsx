import type { ActiveBrand } from "../../lib/useActiveBrand";

/** 品牌 logo；站长没上传 logo 时，用主题色和名称首字。 */
export function BrandMark({ brand, className = "h-8 w-8" }: { brand: ActiveBrand; className?: string }) {
  if (brand.logoUrl) return <img src={brand.logoUrl} alt="" className={`${className} shrink-0 rounded-lg object-cover`} />;
  return (
    <span className={`${className} flex shrink-0 items-center justify-center rounded-lg bg-brand text-sm font-semibold text-brand-foreground`} aria-hidden>
      {brand.name.slice(0, 1)}
    </span>
  );
}
