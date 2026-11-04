import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { CircleCheck, CircleX } from "lucide-react";
import { SetupDialog } from "../tools/SetupDialog";
import { Button } from "../ui/button";
import { listSites, type SwitchReport } from "../../lib/api";
import { errorText } from "../../lib/errors";
import { inDesktop } from "../../lib/tauri";

/** 托盘发来的事件：切换站点的结果显示在顶部；还没配置过工具时，打开一键配置并选中那个站点。 */
export function TrayEvents() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const sites = useQuery({ queryKey: ["sites"], queryFn: listSites });
  const [report, setReport] = useState<SwitchReport | null>(null);
  const [setupSite, setSetupSite] = useState<string | null>(null);

  useEffect(() => {
    if (!inDesktop) return;
    const stops = [
      listen<SwitchReport>("tray-switched", (event) => {
        setReport(event.payload);
        void queryClient.invalidateQueries({ queryKey: ["tools"] });
      }),
      listen<string>("tray-setup", (event) => setSetupSite(event.payload)),
    ];
    return () => {
      for (const stop of stops) void stop.then((unlisten) => unlisten());
    };
  }, [queryClient]);

  // 全部切换成功的提示几秒后自动消失；有失败的留着，等用户看完。
  useEffect(() => {
    if (!report || report.failures.length > 0) return;
    const timer = window.setTimeout(() => setReport(null), 6000);
    return () => window.clearTimeout(timer);
  }, [report]);

  const site = sites.data?.find((item) => item.id === report?.siteId)?.name ?? "";
  const names = (kinds: string[]) => kinds.map((kind) => t(`tools.${kind}`)).join(t("common.separator"));

  return (
    <>
      {report && (
        <div
          role="status"
          className={`flex items-start gap-3 border-b px-8 py-2 text-sm ${report.failures.length > 0 ? "border-danger/20 bg-danger/10" : "border-success/20 bg-success/10"}`}
        >
          {report.failures.length > 0 ? (
            <CircleX className="mt-0.5 h-4 w-4 shrink-0 text-danger" aria-hidden />
          ) : (
            <CircleCheck className="mt-0.5 h-4 w-4 shrink-0 text-success" aria-hidden />
          )}
          <div className="min-w-0 flex-1 space-y-0.5">
            {report.switched.length > 0 && <p>{t("tray.switched", { site, tools: names(report.switched) })}</p>}
            {report.failures.length > 0 && (
              <>
                <p>{t("tray.failed", { site })}</p>
                <ul className="text-danger">
                  {report.failures.map((failure) => (
                    <li key={failure.kind}>{t("tray.failure", { tool: t(`tools.${failure.kind}`), error: errorText(t, failure.error) })}</li>
                  ))}
                </ul>
              </>
            )}
          </div>
          <Button variant="ghost" size="sm" onClick={() => setReport(null)}>
            {t("tray.dismiss")}
          </Button>
        </div>
      )}
      <SetupDialog open={setupSite !== null} initialSiteId={setupSite ?? undefined} onClose={() => setSetupSite(null)} />
    </>
  );
}
