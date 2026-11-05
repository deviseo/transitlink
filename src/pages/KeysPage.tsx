import { Link } from "react-router";
import { useTranslation } from "react-i18next";
import { KeyRound, LoaderCircle } from "lucide-react";
import { PageHeader } from "../components/layout/PageHeader";
import { KeysPanel } from "../components/sites/KeysPanel";
import { ReloginNotice } from "../components/sites/ReloginNotice";
import { buttonVariants } from "../components/ui/button";
import { Card } from "../components/ui/card";
import { siteName, useCurrentSite } from "../lib/currentSite";

/** 密钥：当前站点上的 Key。界面上都是打码的，复制时桌面端直接把完整 Key 写进剪贴板。 */
export function KeysPage() {
  const { t } = useTranslation();
  const { site, loaded } = useCurrentSite();
  if (!loaded) return <LoaderCircle className="h-5 w-5 animate-spin text-muted-foreground" aria-hidden />;
  if (!site) {
    return (
      <div className="space-y-6">
        <PageHeader title={t("keys.title")} />
        <Card className="flex flex-col items-center px-8 py-12 text-center">
          <KeyRound className="h-10 w-10 text-muted-foreground" aria-hidden />
          <p className="mt-4 max-w-md text-sm text-muted-foreground">{t("keys.noSite")}</p>
          <Link to="/sites" className={`${buttonVariants()} mt-6`}>
            {t("home.goSites")}
          </Link>
        </Card>
      </div>
    );
  }
  return (
    <div className="space-y-6">
      <PageHeader title={t("keys.title")} subtitle={t("keys.subtitle", { site: siteName(site) })} />
      <ReloginNotice site={site} />
      <Card>
        <KeysPanel key={site.id} siteId={site.id} />
      </Card>
    </div>
  );
}
