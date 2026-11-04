import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { PageHeader } from "../components/layout/PageHeader";
import { Button } from "../components/ui/button";
import { Card } from "../components/ui/card";
import { InfoRow } from "../components/ui/info-row";
import { languages } from "../i18n";
import { getAppInfo } from "../lib/api";

export function SettingsPage() {
  const { t, i18n } = useTranslation();
  const { data: info } = useQuery({ queryKey: ["app-info"], queryFn: getAppInfo });

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
      </Card>
      <Card>
        <h2 className="font-medium">{t("settings.about")}</h2>
        <dl className="mt-3 space-y-2 text-sm">
          <InfoRow label={t("settings.version")} value={info?.version} />
          <InfoRow label={t("settings.platform")} value={info?.platform} />
          <InfoRow label={t("settings.dataDir")} value={info?.dataDir} mono />
        </dl>
      </Card>
    </div>
  );
}
