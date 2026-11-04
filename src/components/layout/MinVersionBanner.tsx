import { useTranslation } from "react-i18next";
import { useMutation, useQuery } from "@tanstack/react-query";
import { ExternalLink, LoaderCircle, TriangleAlert } from "lucide-react";
import { Button } from "../ui/button";
import { checkUpdate, installUpdate } from "../../lib/api";
import { errorText } from "../../lib/errors";
import { useMinVersionRequirement } from "../../lib/minVersion";
import { inDesktop } from "../../lib/tauri";

/** 下载最新版的页面（没有可用的应用内更新时用）。 */
const downloadUrl = "https://github.com/deviseo/transitlink/releases/latest";

/** 站长要求的最低版本比现在的版本高时，在顶部提示更新；这条提示不能关闭。 */
export function MinVersionBanner() {
  const { t } = useTranslation();
  const requirement = useMinVersionRequirement();
  const update = useQuery({ queryKey: ["update"], queryFn: checkUpdate, enabled: inDesktop, staleTime: Infinity, retry: false });
  const install = useMutation({ mutationFn: installUpdate });
  if (!requirement) return null;

  return (
    <div role="alert" className="flex flex-wrap items-center gap-3 border-b border-danger/30 bg-danger/10 px-8 py-2 text-sm">
      <TriangleAlert className="h-4 w-4 shrink-0 text-danger" aria-hidden />
      <span className="min-w-0 flex-1">{t("update.required", { site: requirement.site, version: requirement.version, current: requirement.current })}</span>
      {install.isError && <span className="text-xs text-danger">{errorText(t, install.error)}</span>}
      {update.data ? (
        <Button size="sm" onClick={() => install.mutate()} disabled={install.isPending}>
          {install.isPending && <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />}
          {install.isPending ? t("update.installing") : t("update.install")}
        </Button>
      ) : (
        <a href={downloadUrl} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 text-sm font-medium text-brand hover:underline">
          {t("update.download")}
          <ExternalLink className="h-3.5 w-3.5" aria-hidden />
        </a>
      )}
    </div>
  );
}
