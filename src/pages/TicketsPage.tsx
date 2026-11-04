import { useState } from "react";
import { Link, useNavigate, useSearchParams } from "react-router";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronRight, LifeBuoy, LoaderCircle, MessagesSquare, Plus, RefreshCw } from "lucide-react";
import { PageHeader } from "../components/layout/PageHeader";
import { NewTicketDialog } from "../components/tickets/NewTicketDialog";
import { StatusBadge, TicketConsent, TicketProblem, siteLabel } from "../components/tickets/parts";
import { Button, buttonVariants } from "../components/ui/button";
import { Card } from "../components/ui/card";
import type { TicketDetail, TicketList } from "../lib/api";
import { cn } from "../lib/cn";
import { relativeTime, syncList, ticketKeys, ticketListQuery, useTicketLists, useTicketSites } from "../lib/tickets";

/** 工单列表：每个开了工单的站点各自一份，可以切换站点；有新回复的工单前面有红点。 */
export function TicketsPage() {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [params, setParams] = useSearchParams();
  const { sites, loaded } = useTicketSites();
  const lists = useTicketLists(sites);
  const site = sites.find((item) => item.id === params.get("site")) ?? sites[0];
  const allowed = site?.ticketsAllowed === true;
  const list = useQuery({ ...ticketListQuery(site?.id ?? ""), enabled: allowed });
  const [creating, setCreating] = useState(false);

  const onCreated = (ticket: TicketDetail) => {
    if (!site) return;
    setCreating(false);
    queryClient.setQueryData(ticketKeys.detail(site.id, ticket.id), ticket);
    queryClient.setQueryData<TicketList>(ticketKeys.list(site.id), (current) => syncList(current, ticket));
    void navigate(`/tickets/${site.id}/${ticket.id}`);
  };

  if (loaded && !site) {
    return (
      <div className="space-y-6">
        <PageHeader title={t("tickets.title")} subtitle={t("tickets.subtitle")} />
        <Card className="flex flex-col items-center px-8 py-12 text-center">
          <LifeBuoy className="h-10 w-10 text-muted-foreground" aria-hidden />
          <h2 className="mt-4 text-lg font-medium">{t("tickets.noSitesTitle")}</h2>
          <p className="mt-2 max-w-md text-sm text-muted-foreground">{t("tickets.noSitesDesc")}</p>
          <Link to="/sites" className={cn(buttonVariants(), "mt-6")}>
            {t("tickets.goSites")}
          </Link>
        </Card>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <PageHeader
        title={t("tickets.title")}
        subtitle={t("tickets.subtitle")}
        action={
          allowed && (
            <div className="flex shrink-0 gap-2">
              <Button variant="secondary" onClick={() => void list.refetch()} disabled={list.isFetching}>
                {list.isFetching ? <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden /> : <RefreshCw className="h-4 w-4" aria-hidden />}
                {t("common.refresh")}
              </Button>
              <Button onClick={() => setCreating(true)}>
                <Plus className="h-4 w-4" aria-hidden />
                {t("tickets.new")}
              </Button>
            </div>
          )
        }
      />

      {site && sites.length > 1 && (
        <div className="flex items-center gap-3">
          <label htmlFor="ticket-site" className="shrink-0 text-sm font-medium">
            {t("tickets.site")}
          </label>
          <select
            id="ticket-site"
            value={site.id}
            onChange={(event) => setParams({ site: event.target.value }, { replace: true })}
            className="h-9 min-w-0 max-w-sm flex-1 rounded-lg border border-border bg-card px-2 text-sm text-foreground outline-none focus:border-brand focus:ring-1 focus:ring-brand"
          >
            {sites.map((item, index) => {
              const unread = lists[index]?.data?.unread ?? 0;
              return (
                <option key={item.id} value={item.id}>
                  {unread > 0 ? `${siteLabel(item)} · ${t("tickets.unreadCount", { count: unread })}` : siteLabel(item)}
                </option>
              );
            })}
          </select>
        </div>
      )}

      {site && !allowed && <TicketConsent site={site} />}
      {(!loaded || list.isLoading) && <LoaderCircle className="h-5 w-5 animate-spin text-muted-foreground" aria-hidden />}
      {list.isError && <TicketProblem error={list.error} onRetry={() => void list.refetch()} />}

      {list.data && list.data.items.length === 0 && (
        <Card className="flex flex-col items-center px-8 py-12 text-center">
          <MessagesSquare className="h-10 w-10 text-muted-foreground" aria-hidden />
          <h2 className="mt-4 text-lg font-medium">{t("tickets.empty")}</h2>
          <p className="mt-2 max-w-md text-sm text-muted-foreground">{t("tickets.emptyDesc")}</p>
          <Button className="mt-6" onClick={() => setCreating(true)}>
            <Plus className="h-4 w-4" aria-hidden />
            {t("tickets.new")}
          </Button>
        </Card>
      )}

      {site && list.data && list.data.items.length > 0 && (
        <Card className="overflow-hidden p-0">
          <ul className="divide-y divide-border">
            {list.data.items.map((ticket) => (
              <li key={ticket.id}>
                <Link
                  to={`/tickets/${site.id}/${ticket.id}`}
                  className="flex items-center gap-3 px-5 py-4 transition-colors hover:bg-muted/60 focus-visible:bg-muted/60 focus-visible:outline-none"
                >
                  <span className={cn("h-2 w-2 shrink-0 rounded-full", ticket.unread ? "bg-danger" : "bg-transparent")} aria-hidden />
                  <div className="min-w-0 flex-1">
                    <p className={cn("truncate text-sm", ticket.unread ? "font-semibold" : "font-medium")}>
                      {ticket.title}
                      {ticket.unread && <span className="sr-only"> · {t("tickets.unread")}</span>}
                    </p>
                    <p className="mt-0.5 truncate text-xs text-muted-foreground">
                      {ticket.category} · {t("tickets.updated", { time: relativeTime(ticket.lastMessageAt, i18n.language) })}
                    </p>
                  </div>
                  <StatusBadge status={ticket.status} />
                  <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground" aria-hidden />
                </Link>
              </li>
            ))}
          </ul>
        </Card>
      )}

      {creating && site && allowed && <NewTicketDialog site={site} onClose={() => setCreating(false)} onCreated={onCreated} />}
    </div>
  );
}
