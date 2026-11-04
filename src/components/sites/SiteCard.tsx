import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { ChevronDown, LoaderCircle, Trash2 } from "lucide-react";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card } from "../ui/card";
import { KeysPanel } from "./KeysPanel";
import { SiteAvatar } from "./SiteAvatar";
import { isCommandError, siteAccount, type SiteView } from "../../lib/api";
import { errorText } from "../../lib/errors";

/** 一个已接入的站点：余额、Key 管理、删除；登录失效时提示重新登录。 */
export function SiteCard({ site, onRelogin, onRemove }: { site: SiteView; onRelogin: (site: SiteView) => void; onRemove: (site: SiteView) => void }) {
  const { t, i18n } = useTranslation();
  const account = useQuery({ queryKey: ["site-account", site.id], queryFn: () => siteAccount(site.id) });
  const [showKeys, setShowKeys] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const expired = isCommandError(account.error) && account.error.code === "sessionExpired";
  const money = new Intl.NumberFormat(i18n.language, { style: "currency", currency: "USD", maximumFractionDigits: 2 });

  return (
    <Card>
      <div className="flex items-start gap-3">
        <SiteAvatar name={site.name} logoUrl={site.logoUrl} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <h2 className="truncate font-medium">{site.name}</h2>
            <Badge tone="brand">{site.platform === "newapi" ? "new-api" : "sub2api"}</Badge>
          </div>
          <p className="truncate text-xs text-muted-foreground">
            {site.baseUrl} · {site.account}
          </p>
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

      <div className="mt-3 flex items-center justify-between gap-2">
        <Button variant="ghost" size="sm" onClick={() => setShowKeys((value) => !value)} aria-expanded={showKeys} disabled={expired}>
          <ChevronDown className={`h-4 w-4 transition-transform ${showKeys ? "rotate-180" : ""}`} aria-hidden />
          {t("sites.card.keys")}
        </Button>
        {confirming ? (
          <div className="flex items-center gap-2 text-sm">
            <span className="text-muted-foreground">{t("sites.card.removeConfirm")}</span>
            <Button variant="ghost" size="sm" onClick={() => setConfirming(false)}>
              {t("common.cancel")}
            </Button>
            <Button size="sm" className="bg-danger" onClick={() => onRemove(site)}>
              {t("sites.card.remove")}
            </Button>
          </div>
        ) : (
          <Button variant="ghost" size="sm" onClick={() => setConfirming(true)}>
            <Trash2 className="h-4 w-4" aria-hidden />
            {t("sites.card.remove")}
          </Button>
        )}
      </div>

      {showKeys && !expired && <KeysPanel siteId={site.id} />}
    </Card>
  );
}
