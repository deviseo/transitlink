import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { ArrowRightLeft, Info, LoaderCircle, Trash2 } from "lucide-react";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card } from "../ui/card";
import { SiteAvatar } from "./SiteAvatar";
import { isCommandError, siteAccount, type SiteView } from "../../lib/api";
import { siteName } from "../../lib/currentSite";
import { errorText } from "../../lib/errors";

/** 管理站点里的一个站点：余额、切换、删除；登录失效时提示重新登录；没用专属链接接入的说明少了什么。 */
export function SiteCard({
  site,
  current,
  onSwitch,
  onRelogin,
  onRemove,
}: {
  site: SiteView;
  /** 是不是左上角的当前站点。 */
  current: boolean;
  onSwitch: (site: SiteView) => void;
  onRelogin: (site: SiteView) => void;
  onRemove: (site: SiteView) => void;
}) {
  const { t, i18n } = useTranslation();
  const account = useQuery({ queryKey: ["site-account", site.id], queryFn: () => siteAccount(site.id) });
  const [confirming, setConfirming] = useState(false);
  const expired = isCommandError(account.error) && account.error.code === "sessionExpired";
  const money = new Intl.NumberFormat(i18n.language, { style: "currency", currency: "USD", maximumFractionDigits: 2 });

  return (
    <Card className={current ? "border-brand/60" : undefined}>
      <div className="flex items-start gap-3">
        <SiteAvatar name={siteName(site)} logoUrl={site.config?.brand.logoUrl || site.logoUrl} />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="truncate font-medium">{siteName(site)}</h2>
            <Badge tone="brand">{site.platform === "newapi" ? "new-api" : "sub2api"}</Badge>
            {current && <Badge tone="success">{t("sites.current")}</Badge>}
          </div>
          <p className="truncate text-xs text-muted-foreground">
            {site.baseUrl} · {site.account}
          </p>
          {site.config ? (
            <p className="truncate text-xs text-brand">{t("sites.card.linked")}</p>
          ) : (
            <p className="flex items-center gap-1 truncate text-xs text-muted-foreground" title={t("sites.notLinked")}>
              <Info className="h-3 w-3 shrink-0" aria-hidden />
              {t("sites.card.notLinkedShort")}
            </p>
          )}
        </div>
        <div className="text-right">
          <p className="text-xs text-muted-foreground">{t("sites.card.balance")}</p>
          {account.isLoading && <LoaderCircle className="ml-auto mt-1 h-4 w-4 animate-spin text-muted-foreground" aria-hidden />}
          {account.data && <p className="text-lg font-semibold tabular-nums">{money.format(account.data.balanceUsd)}</p>}
          {account.isError && !expired && <p className="max-w-56 text-xs text-danger">{errorText(t, account.error)}</p>}
        </div>
      </div>

      {expired && (
        <div className="mt-3 flex items-center justify-between gap-3 rounded-lg bg-danger/10 p-3 text-sm text-danger">
          {t("sites.card.expired")}
          <Button size="sm" onClick={() => onRelogin(site)}>
            {t("sites.card.relogin")}
          </Button>
        </div>
      )}

      <div className="mt-4 flex flex-wrap items-center justify-end gap-2 border-t border-border/70 pt-3">
        {confirming ? (
          <div className="flex items-center gap-2 text-sm">
            <span className="text-muted-foreground">{t("sites.card.removeConfirm")}</span>
            <Button variant="ghost" size="sm" onClick={() => setConfirming(false)}>
              {t("common.cancel")}
            </Button>
            <Button size="sm" variant="danger" onClick={() => onRemove(site)}>
              {t("sites.card.remove")}
            </Button>
          </div>
        ) : (
          <>
            <Button variant="ghost" size="sm" onClick={() => setConfirming(true)}>
              <Trash2 className="h-4 w-4" aria-hidden />
              {t("sites.card.remove")}
            </Button>
            {!current && (
              <Button variant="secondary" size="sm" onClick={() => onSwitch(site)}>
                <ArrowRightLeft className="h-4 w-4" aria-hidden />
                {t("sites.switchTo")}
              </Button>
            )}
          </>
        )}
      </div>
    </Card>
  );
}
