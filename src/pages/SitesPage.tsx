import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Globe, Link2, Plus } from "lucide-react";
import { PageHeader } from "../components/layout/PageHeader";
import { useSiteSwitch } from "../components/layout/SiteSwitch";
import { AddSiteDialog } from "../components/sites/AddSiteDialog";
import { SiteCard } from "../components/sites/SiteCard";
import { SetupDialog } from "../components/tools/SetupDialog";
import { Button } from "../components/ui/button";
import { Card } from "../components/ui/card";
import { listSites, removeSite, type SiteView } from "../lib/api";
import { settingsKey, useCurrentSite } from "../lib/currentSite";

/** 管理站点：接入、重新登录、删除；切换当前站点（也可以用左上角的站点名称切换）。 */
export function SitesPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const sites = useQuery({ queryKey: ["sites"], queryFn: listSites });
  const { site: current } = useCurrentSite();
  const { switchTo } = useSiteSwitch();
  const [dialog, setDialog] = useState<{ url: string; account: string } | null>(null);
  // 用专属链接接入后，接着打开一键配置。
  const [setupSite, setSetupSite] = useState<string | null>(null);

  const refresh = (site?: SiteView) => {
    void queryClient.invalidateQueries({ queryKey: ["sites"] });
    if (site) {
      void queryClient.invalidateQueries({ queryKey: ["site-account", site.id] });
      void queryClient.invalidateQueries({ queryKey: ["site-keys", site.id] });
    }
  };

  const onRemove = async (site: SiteView) => {
    await removeSite(site.id);
    refresh();
    // 删的是当前站点时，桌面端清掉了设置，界面换回自动挑选。
    void queryClient.invalidateQueries({ queryKey: settingsKey });
  };

  const addButton = (
    <Button onClick={() => setDialog({ url: "", account: "" })}>
      <Plus className="h-4 w-4" aria-hidden />
      {t("sites.add.button")}
    </Button>
  );

  return (
    <div className="space-y-6">
      <PageHeader title={t("sites.title")} subtitle={t("sites.subtitle")} action={sites.data && sites.data.length > 0 ? addButton : undefined} />

      {sites.data && sites.data.length === 0 && (
        <Card className="flex flex-col items-center px-8 py-12 text-center">
          <Globe className="h-10 w-10 text-muted-foreground" aria-hidden />
          <h2 className="mt-4 text-lg font-medium">{t("sites.emptyTitle")}</h2>
          <p className="mt-2 text-sm text-muted-foreground">{t("sites.emptyDesc")}</p>
          <ul className="mt-3 max-w-lg space-y-2 text-left text-sm">
            <li className="flex gap-2">
              <Globe className="mt-0.5 h-4 w-4 shrink-0 text-brand" aria-hidden />
              {t("sites.wayUrl")}
            </li>
            <li className="flex gap-2">
              <Link2 className="mt-0.5 h-4 w-4 shrink-0 text-brand" aria-hidden />
              {t("sites.wayLink")}
            </li>
          </ul>
          <div className="mt-6">{addButton}</div>
        </Card>
      )}

      <div className="space-y-4">
        {(sites.data ?? []).map((site) => (
          <SiteCard
            key={site.id}
            site={site}
            current={site.id === current?.id}
            onSwitch={switchTo}
            onRelogin={(item) => setDialog({ url: item.baseUrl, account: item.account })}
            onRemove={(item) => void onRemove(item)}
          />
        ))}
      </div>

      <AddSiteDialog
        open={dialog !== null}
        initialUrl={dialog?.url}
        initialAccount={dialog?.account}
        onClose={() => setDialog(null)}
        onAdded={(site, viaLink) => {
          setDialog(null);
          refresh(site);
          if (viaLink) setSetupSite(site.id);
        }}
      />
      <SetupDialog open={setupSite !== null} initialSiteId={setupSite ?? undefined} onClose={() => setSetupSite(null)} />
    </div>
  );
}
