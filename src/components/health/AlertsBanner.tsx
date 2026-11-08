import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { BellRing } from "lucide-react";
import { FindingRow, useFixer } from "./findings";
import { Button } from "../ui/button";
import { getAlerts, type Alert } from "../../lib/health";
import { inDesktop } from "../../lib/tauri";

/** 一次最多显示几条，其余的在体检里看。 */
const MAX_SHOWN = 3;

/** 关掉的是哪一组提醒（提醒变了会再显示）；只记到应用关掉为止。 */
const dismissedKey = "transitlink.alerts.dismissed";

/**
 * 顶部的余额、Key 提醒：桌面端每 30 分钟查一次，有变化时发 alerts-changed 事件。每条提醒说清楚是哪个站点、
 * 哪个工具，带「去充值」「换个 Key」这类按钮。
 */
export function AlertsBanner() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const alerts = useQuery({ queryKey: ["alerts"], queryFn: getAlerts });
  const fixer = useFixer();
  const [dismissed, setDismissed] = useState(() => sessionStorage.getItem(dismissedKey) ?? "");

  useEffect(() => {
    if (!inDesktop) return;
    const stop = listen<Alert[]>("alerts-changed", (event) => queryClient.setQueryData(["alerts"], event.payload));
    return () => void stop.then((unlisten) => unlisten());
  }, [queryClient]);

  const list = alerts.data ?? [];
  const signature = list.map((alert) => `${alert.id}:${alert.code}`).join("|");
  if (list.length === 0 || signature === dismissed) return null;
  const error = list.some((alert) => alert.level === "error");

  const dismiss = () => {
    sessionStorage.setItem(dismissedKey, signature);
    setDismissed(signature);
  };

  return (
    <div role="status" className={`flex items-start gap-3 border-b px-8 py-2.5 ${error ? "border-danger/20 bg-danger/5" : "border-warning/20 bg-warning/10"}`}>
      <BellRing className={`mt-0.5 h-4 w-4 shrink-0 ${error ? "text-danger" : "text-warning"}`} aria-hidden />
      <ul className="min-w-0 flex-1 space-y-1.5">
        {list.slice(0, MAX_SHOWN).map((alert) => (
          <FindingRow
            key={alert.id}
            finding={alert}
            fixer={fixer}
            prefix={t("alerts.subject", { subject: alert.kind ? t("alerts.toolOnSite", { tool: t(`tools.${alert.kind}`), site: alert.siteName }) : alert.siteName })}
          />
        ))}
        {list.length > MAX_SHOWN && <li className="pl-6 text-xs text-muted-foreground">{t("alerts.more", { count: list.length - MAX_SHOWN })}</li>}
      </ul>
      <Button variant="ghost" size="sm" className="h-7 px-2 text-xs" onClick={dismiss}>
        {t("alerts.dismiss")}
      </Button>
      {fixer.dialogs}
    </div>
  );
}
