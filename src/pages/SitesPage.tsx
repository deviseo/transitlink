import { useTranslation } from "react-i18next";
import { Globe, Link2 } from "lucide-react";
import { PageHeader } from "../components/layout/PageHeader";
import { Badge } from "../components/ui/badge";
import { Card } from "../components/ui/card";

export function SitesPage() {
  const { t } = useTranslation();
  return (
    <div className="space-y-6">
      <PageHeader title={t("sites.title")} subtitle={t("sites.subtitle")} />
      <Card className="flex flex-col items-center px-8 py-12 text-center">
        <Globe className="h-10 w-10 text-muted-foreground" aria-hidden />
        <h2 className="mt-4 text-lg font-medium">{t("sites.emptyTitle")}</h2>
        <p className="mt-2 text-sm text-muted-foreground">{t("sites.emptyDesc")}</p>
        <ul className="mt-3 max-w-lg space-y-2 text-left text-sm">
          <li className="flex gap-2">
            <Link2 className="mt-0.5 h-4 w-4 shrink-0 text-brand" aria-hidden />
            {t("sites.wayLink")}
          </li>
          <li className="flex gap-2">
            <Globe className="mt-0.5 h-4 w-4 shrink-0 text-brand" aria-hidden />
            {t("sites.wayUrl")}
          </li>
        </ul>
        <Badge tone="brand" className="mt-6">
          {t("common.comingSoon")}
        </Badge>
      </Card>
    </div>
  );
}
