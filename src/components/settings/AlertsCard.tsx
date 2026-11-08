import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { BellRing } from "lucide-react";
import { SectionCard } from "../ui/card";
import { Input } from "../ui/input";
import { Switch } from "../ui/switch";
import { getAppSettings, setAlertSettings, type AlertSettings } from "../../lib/api";
import { settingsKey } from "../../lib/currentSite";
import { errorText } from "../../lib/errors";

function Row({ title, description, children }: { title: string; description?: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-6 py-3 first:pt-0 last:pb-0">
      <div className="min-w-0">
        <p className="text-[13px] font-medium">{title}</p>
        {description && <p className="mt-0.5 text-xs text-muted-foreground">{description}</p>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

/** 设置里的提醒：余额低于多少时提醒，Key 不能用时提醒。改完桌面端马上按新设置查一次。 */
export function AlertsCard() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const settings = useQuery({ queryKey: settingsKey, queryFn: getAppSettings });
  const save = useMutation({
    mutationFn: setAlertSettings,
    onSuccess: (next) => queryClient.setQueryData(settingsKey, next),
  });
  const alerts = settings.data?.alerts;
  const [threshold, setThreshold] = useState("");
  // 只在保存的值变了时同步（重新读设置时不要冲掉正在输入的数）。
  const saved = alerts?.balanceBelow;
  useEffect(() => {
    if (saved !== undefined) setThreshold(String(saved));
  }, [saved]);
  if (!alerts) return null;

  const change = (patch: Partial<AlertSettings>) => save.mutate({ ...alerts, ...patch });
  const commitThreshold = () => {
    const value = Number(threshold);
    if (!Number.isFinite(value) || value < 0) {
      setThreshold(String(alerts.balanceBelow));
      return;
    }
    if (value !== alerts.balanceBelow) change({ balanceBelow: value });
  };

  return (
    <SectionCard title={t("settings.alerts.title")} description={t("settings.alerts.description")} icon={<BellRing className="h-4 w-4" aria-hidden />}>
      <div className="divide-y divide-border/70">
        <Row title={t("settings.alerts.enabled")} description={t("settings.alerts.enabledHint")}>
          <Switch label={t("settings.alerts.enabled")} checked={alerts.enabled} disabled={save.isPending} onChange={(enabled) => change({ enabled })} />
        </Row>
        <Row title={t("settings.alerts.threshold")} description={t("settings.alerts.thresholdHint")}>
          <label className="flex items-center gap-1.5 text-[13px] text-muted-foreground">
            $
            <Input
              type="number"
              min={0}
              step={0.5}
              inputMode="decimal"
              aria-label={t("settings.alerts.threshold")}
              value={threshold}
              disabled={!alerts.enabled || save.isPending}
              onChange={(event) => setThreshold(event.target.value)}
              onBlur={commitThreshold}
              onKeyDown={(event) => event.key === "Enter" && commitThreshold()}
              className="h-8 w-24 text-right tabular-nums"
            />
          </label>
        </Row>
        <Row title={t("settings.alerts.keys")} description={t("settings.alerts.keysHint")}>
          <Switch label={t("settings.alerts.keys")} checked={alerts.keys} disabled={!alerts.enabled || save.isPending} onChange={(keys) => change({ keys })} />
        </Row>
      </div>
      {save.isError && <p className="mt-2 text-[13px] text-danger">{errorText(t, save.error)}</p>}
    </SectionCard>
  );
}
