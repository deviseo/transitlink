import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CircleCheck, Download, LoaderCircle, RefreshCw } from "lucide-react";
import { PageHeader } from "../components/layout/PageHeader";
import { Button } from "../components/ui/button";
import { SectionCard } from "../components/ui/card";
import { Segmented } from "../components/ui/segmented";
import { Switch } from "../components/ui/switch";
import { languages } from "../i18n";
import { checkUpdate, getAppInfo, getAppSettings, installUpdate, setCloseToTray } from "../lib/api";
import { errorText } from "../lib/errors";

/** 设置里的一行：左边是名称和说明，右边是控件。 */
function SettingRow({ title, description, children }: { title: string; description?: string; children: ReactNode }) {
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

export function SettingsPage() {
  const { t, i18n } = useTranslation();
  const { data: info } = useQuery({ queryKey: ["app-info"], queryFn: getAppInfo });
  const queryClient = useQueryClient();
  const settings = useQuery({ queryKey: ["app-settings"], queryFn: getAppSettings });
  const closeToTray = useMutation({
    mutationFn: setCloseToTray,
    onSuccess: (next) => queryClient.setQueryData(["app-settings"], next),
  });

  return (
    <div className="space-y-5">
      <PageHeader title={t("settings.title")} />
      <SectionCard title={t("settings.general")}>
        <div className="divide-y divide-border/70">
          <SettingRow title={t("settings.language")}>
            <Segmented
              label={t("settings.language")}
              value={i18n.language}
              onChange={(code) => void i18n.changeLanguage(code)}
              options={languages.map((language) => ({ value: language.code, label: language.label }))}
            />
          </SettingRow>
          {settings.data && (
            <SettingRow title={t("settings.closeToTrayTitle")} description={t("settings.closeToTray")}>
              <Switch
                label={t("settings.closeToTrayTitle")}
                checked={settings.data.closeToTray}
                disabled={closeToTray.isPending}
                onChange={(checked) => closeToTray.mutate(checked)}
              />
            </SettingRow>
          )}
        </div>
        {closeToTray.isError && <p className="mt-2 text-[13px] text-danger">{errorText(t, closeToTray.error)}</p>}
      </SectionCard>
      <SectionCard title={t("settings.about")}>
        <div className="divide-y divide-border/70">
          <SettingRow title={t("settings.version")} description={info ? `TransitLink ${info.version}` : undefined}>
            <UpdateCheck />
          </SettingRow>
          <SettingRow title={t("settings.platform")}>
            <span className="text-[13px] text-muted-foreground">{info?.platform ?? "—"}</span>
          </SettingRow>
          <SettingRow title={t("settings.dataDir")}>
            <code className="font-mono text-xs text-muted-foreground">{info?.dataDir ?? "—"}</code>
          </SettingRow>
        </div>
      </SectionCard>
    </div>
  );
}

/** 手动检查更新；有新版本时可以直接更新并重启。 */
function UpdateCheck() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const check = useMutation({
    mutationFn: checkUpdate,
    // 让顶部的更新提示也显示出来。
    onSuccess: (update) => queryClient.setQueryData(["update"], update),
  });
  const install = useMutation({ mutationFn: installUpdate });

  return (
    <div className="flex flex-wrap items-center justify-end gap-2 text-[13px]">
      {check.isSuccess && check.data === null && (
        <span className="flex items-center gap-1.5 text-success">
          <CircleCheck className="h-4 w-4" aria-hidden />
          {t("update.upToDate")}
        </span>
      )}
      {check.isError && <span className="text-danger">{errorText(t, check.error)}</span>}
      {install.isError && <span className="text-danger">{errorText(t, install.error)}</span>}
      {check.data ? (
        <>
          <span>{t("update.found", { version: check.data.version })}</span>
          <Button size="sm" onClick={() => install.mutate()} disabled={install.isPending}>
            {install.isPending ? <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden /> : <Download className="h-4 w-4" aria-hidden />}
            {install.isPending ? t("update.installing") : t("update.install")}
          </Button>
        </>
      ) : (
        <Button variant="secondary" size="sm" onClick={() => check.mutate()} disabled={check.isPending || install.isPending}>
          {check.isPending ? <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden /> : <RefreshCw className="h-4 w-4" aria-hidden />}
          {check.isPending ? t("update.checking") : t("update.check")}
        </Button>
      )}
    </div>
  );
}
