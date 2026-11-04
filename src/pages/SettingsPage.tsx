import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CircleCheck, Download, LoaderCircle, RefreshCw } from "lucide-react";
import { PageHeader } from "../components/layout/PageHeader";
import { Button } from "../components/ui/button";
import { Card } from "../components/ui/card";
import { InfoRow } from "../components/ui/info-row";
import { languages } from "../i18n";
import { checkUpdate, getAppInfo, getAppSettings, installUpdate, setCloseToTray } from "../lib/api";
import { errorText } from "../lib/errors";

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
    <div className="space-y-6">
      <PageHeader title={t("settings.title")} />
      <Card>
        <h2 className="font-medium">{t("settings.language")}</h2>
        <div className="mt-3 flex gap-2">
          {languages.map((language) => (
            <Button
              key={language.code}
              size="sm"
              variant={i18n.language === language.code ? "primary" : "secondary"}
              aria-pressed={i18n.language === language.code}
              onClick={() => void i18n.changeLanguage(language.code)}
            >
              {language.label}
            </Button>
          ))}
        </div>
        {settings.data && (
          <label className="mt-4 flex items-start gap-2.5 border-t border-border pt-4 text-sm">
            <input
              type="checkbox"
              className="mt-0.5 h-4 w-4 shrink-0 accent-brand"
              checked={settings.data.closeToTray}
              disabled={closeToTray.isPending}
              onChange={(event) => closeToTray.mutate(event.target.checked)}
            />
            {t("settings.closeToTray")}
          </label>
        )}
        {closeToTray.isError && <p className="mt-2 text-sm text-danger">{errorText(t, closeToTray.error)}</p>}
      </Card>
      <Card>
        <h2 className="font-medium">{t("settings.about")}</h2>
        <dl className="mt-3 space-y-2 text-sm">
          <InfoRow label={t("settings.version")} value={info?.version} />
          <InfoRow label={t("settings.platform")} value={info?.platform} />
          <InfoRow label={t("settings.dataDir")} value={info?.dataDir} mono />
        </dl>
        <UpdateCheck />
      </Card>
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
    <div className="mt-4 flex flex-wrap items-center gap-3 border-t border-border pt-4 text-sm">
      <Button variant="secondary" size="sm" onClick={() => check.mutate()} disabled={check.isPending || install.isPending}>
        {check.isPending ? <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden /> : <RefreshCw className="h-4 w-4" aria-hidden />}
        {check.isPending ? t("update.checking") : t("update.check")}
      </Button>
      {check.isSuccess && check.data === null && (
        <span className="flex items-center gap-1.5 text-success">
          <CircleCheck className="h-4 w-4" aria-hidden />
          {t("update.upToDate")}
        </span>
      )}
      {check.data && (
        <>
          <span>{t("update.found", { version: check.data.version })}</span>
          <Button size="sm" onClick={() => install.mutate()} disabled={install.isPending}>
            {install.isPending ? <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden /> : <Download className="h-4 w-4" aria-hidden />}
            {install.isPending ? t("update.installing") : t("update.install")}
          </Button>
        </>
      )}
      {check.isError && <span className="text-danger">{errorText(t, check.error)}</span>}
      {install.isError && <span className="text-danger">{errorText(t, install.error)}</span>}
    </div>
  );
}
