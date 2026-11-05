import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { CircleAlert } from "lucide-react";
import { AddSiteDialog } from "./AddSiteDialog";
import { Button } from "../ui/button";
import { isCommandError, siteAccount, type SiteView } from "../../lib/api";

/** 站点的登录失效了：提示并可以直接重新登录（用的是和概览、站点卡片同一份账户信息）。 */
export function ReloginNotice({ site }: { site: SiteView }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const account = useQuery({ queryKey: ["site-account", site.id], queryFn: () => siteAccount(site.id) });
  const [open, setOpen] = useState(false);
  if (!(isCommandError(account.error) && account.error.code === "sessionExpired")) return null;
  return (
    <div role="alert" className="flex flex-wrap items-center gap-3 rounded-lg bg-danger/10 p-3 text-sm text-danger">
      <CircleAlert className="h-4 w-4 shrink-0" aria-hidden />
      <span className="min-w-0 flex-1">{t("sites.card.expired")}</span>
      <Button size="sm" onClick={() => setOpen(true)}>
        {t("sites.card.relogin")}
      </Button>
      <AddSiteDialog
        open={open}
        initialUrl={site.baseUrl}
        initialAccount={site.account}
        onClose={() => setOpen(false)}
        onAdded={() => {
          setOpen(false);
          void queryClient.invalidateQueries({ queryKey: ["sites"] });
          void queryClient.invalidateQueries({ queryKey: ["site-account", site.id] });
          void queryClient.invalidateQueries({ queryKey: ["site-keys", site.id] });
        }}
      />
    </div>
  );
}
