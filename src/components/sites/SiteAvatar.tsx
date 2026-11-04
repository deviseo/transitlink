import { useState } from "react";
import { cn } from "../../lib/cn";

/** 站点 logo；没有或加载失败时显示名称首字。 */
export function SiteAvatar({ name, logoUrl, className }: { name: string; logoUrl: string | null; className?: string }) {
  const [failed, setFailed] = useState(false);
  if (logoUrl && !failed) {
    return <img src={logoUrl} alt="" onError={() => setFailed(true)} className={cn("h-9 w-9 shrink-0 rounded-lg object-contain", className)} />;
  }
  return (
    <span aria-hidden className={cn("flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-brand/10 text-sm font-semibold text-brand", className)}>
      {name.trim().slice(0, 1).toUpperCase() || "?"}
    </span>
  );
}
