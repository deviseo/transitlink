import { Link } from "react-router";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { CircleAlert, LoaderCircle, RefreshCw, ShieldCheck } from "lucide-react";
import { Badge, type Tone } from "../ui/badge";
import { Button, buttonVariants } from "../ui/button";
import { Card } from "../ui/card";
import { allowTickets, isCommandError, type SiteView, type TicketStatus } from "../../lib/api";
import { cn } from "../../lib/cn";
import { errorText } from "../../lib/errors";

const statusTones: Record<TicketStatus, Tone> = { open: "brand", pending: "brand", replied: "success", closed: "neutral" };

/** 工单状态：等客服回复、追问后等回复、客服已回复、已关闭。 */
export function StatusBadge({ status }: { status: TicketStatus }) {
  const { t } = useTranslation();
  return <Badge tone={statusTones[status] ?? "neutral"}>{t(`tickets.status.${status}`, { defaultValue: status })}</Badge>;
}

/** 工单出错时的提示。站点登录失效时给一个去重新登录的入口，还没同意时重新读站点（显示同意卡片），其他错误可以重试。 */
export function TicketProblem({ error, onRetry }: { error: unknown; onRetry?: () => void }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const relogin = isCommandError(error) && error.code === "ticketsSiteLoginExpired";
  const consent = isCommandError(error) && error.code === "ticketsConsentRequired";
  return (
    <div role="alert" className="flex flex-wrap items-center gap-3 rounded-lg bg-danger/10 p-3 text-sm text-danger">
      <CircleAlert className="h-4 w-4 shrink-0" aria-hidden />
      <span className="min-w-0 flex-1">{errorText(t, error)}</span>
      {relogin ? (
        <Link to="/sites" className={buttonVariants({ size: "sm" })}>
          {t("tickets.relogin")}
        </Link>
      ) : consent ? (
        <Button size="sm" onClick={() => void queryClient.invalidateQueries({ queryKey: ["sites"] })}>
          {t("tickets.consent.review")}
        </Button>
      ) : (
        onRetry && (
          <Button variant="secondary" size="sm" onClick={onRetry}>
            <RefreshCw className="h-4 w-4" aria-hidden />
            {t("tickets.retry")}
          </Button>
        )
      )}
    </div>
  );
}

/**
 * 第一次用一个站点的工单前请用户确认：工单由专属链接里站长的 TransitHub 处理，它要用站点的登录核实身份。
 * 专属链接可能不是站长发的，所以同意前不发任何工单请求（也不查未读）。
 */
export function TicketConsent({ site }: { site: SiteView }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const allow = useMutation({
    mutationFn: () => allowTickets(site.id),
    onSuccess: (updated) => {
      queryClient.setQueryData<SiteView[]>(["sites"], (sites) => sites?.map((item) => (item.id === updated.id ? updated : item)));
      void queryClient.invalidateQueries({ queryKey: ["sites"] });
    },
  });
  return (
    <Card className="space-y-4">
      <div className="flex items-start gap-3">
        <ShieldCheck className="mt-0.5 h-5 w-5 shrink-0 text-brand" aria-hidden />
        <div className="min-w-0 space-y-2">
          <h2 className="font-medium">{t("tickets.consent.title")}</h2>
          <p className="text-sm text-muted-foreground">
            {t("tickets.consent.body", { site: site.config?.brand.name || site.name, host: site.ticketsHost ?? "" })}
          </p>
          <p className="text-sm text-muted-foreground">{t("tickets.consent.trust")}</p>
        </div>
      </div>
      {allow.isError && <TicketProblem error={allow.error} />}
      <div className="flex justify-end">
        <Button onClick={() => allow.mutate()} disabled={allow.isPending}>
          {allow.isPending && <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />}
          {t("tickets.consent.allow")}
        </Button>
      </div>
    </Card>
  );
}

/** 字数和上限，写满时变成红色。 */
export function CharCount({ value, max }: { value: string; max: number }) {
  return <span className={cn("text-xs tabular-nums", value.length >= max ? "text-danger" : "text-muted-foreground")}>{`${value.length}/${max}`}</span>;
}

/** 站点在工单页里的名字：站长的品牌名和账号。 */
export function siteLabel(site: SiteView): string {
  return `${site.config?.brand.name || site.name} · ${site.account}`;
}
