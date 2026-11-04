import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Download, LoaderCircle } from "lucide-react";
import { Button } from "../ui/button";
import { checkUpdate, installUpdate } from "../../lib/api";
import { errorText } from "../../lib/errors";
import { useMinVersionRequirement } from "../../lib/minVersion";
import { inDesktop } from "../../lib/tauri";

/** 启动时检查一次更新；有新版本就在顶部提示，可以直接更新并重启。检查失败（例如没联网）不打扰用户。 */
export function UpdateBanner() {
  const { t } = useTranslation();
  const [dismissed, setDismissed] = useState(false);
  const update = useQuery({ queryKey: ["update"], queryFn: checkUpdate, enabled: inDesktop, staleTime: Infinity, retry: false });
  const install = useMutation({ mutationFn: installUpdate });
  // 站长要求更新时由 MinVersionBanner 提示（不能关闭），这里不再重复。
  const required = useMinVersionRequirement();
  if (!update.data || dismissed || required) return null;

  return (
    <div role="status" className="flex flex-wrap items-center gap-3 border-b border-brand/20 bg-brand/10 px-8 py-2 text-sm">
      <Download className="h-4 w-4 shrink-0 text-brand" aria-hidden />
      <span className="min-w-0 flex-1">{t("update.available", { version: update.data.version })}</span>
      {install.isError && <span className="text-xs text-danger">{errorText(t, install.error)}</span>}
      <Button size="sm" onClick={() => install.mutate()} disabled={install.isPending}>
        {install.isPending && <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />}
        {install.isPending ? t("update.installing") : t("update.install")}
      </Button>
      {!install.isPending && (
        <Button variant="ghost" size="sm" onClick={() => setDismissed(true)}>
          {t("update.later")}
        </Button>
      )}
    </div>
  );
}
