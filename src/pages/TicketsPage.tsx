import { useState } from "react";
import { Link, useLocation, useNavigate } from "react-router";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronRight, LifeBuoy, LoaderCircle, MessagesSquare, Plus, RefreshCw } from "lucide-react";
import { PageHeader } from "../components/layout/PageHeader";
import { NewTicketDialog } from "../components/tickets/NewTicketDialog";
import type { TicketDraft } from "../components/health/CheckupDialog";
import { StatusBadge, TicketConsent, TicketProblem } from "../components/tickets/parts";
import { Button, buttonVariants } from "../components/ui/button";
import { Card } from "../components/ui/card";
import type { TicketDetail, TicketList } from "../lib/api";
import { cn } from "../lib/cn";
import { siteName, useCurrentSite } from "../lib/currentSite";
import { relativeTime, syncList, ticketKeys, ticketListQuery } from "../lib/tickets";

/** 工单列表：当前站点的工单（换站点用左上角的切换器）；有新回复的工单前面有红点。 */
export function TicketsPage() {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { site: current, loaded } = useCurrentSite();
  const site = current?.config?.tickets?.enabled ? current : undefined;
  const allowed = site?.ticketsAllowed === true;
  const list = useQuery({ ...ticketListQuery(site?.id ?? ""), enabled: allowed });
  // 从体检过来：带着报告直接新建工单（还没确认工单服务时，确认后再打开）。
  const location = useLocation();
  const [draft, setDraft] = useState<TicketDraft | null>(() => (location.state as { draft?: TicketDraft } | null)?.draft ?? null);
  const [creating, setCreating] = useState(draft !== null);
  const closeNew = () => {
    setCreating(false);
    setDraft(null);
    void navigate(".", { replace: true, state: null });
  };

  const onCreated = (ticket: TicketDetail) => {
    if (!site) return;
    setCreating(false);
    setDraft(null);
    // 先清掉这一页的草稿，返回时不会再打开一个填好的新工单。
    void navigate(".", { replace: true, state: null });
    queryClient.setQueryData(ticketKeys.detail(site.id, ticket.id), ticket);
    queryClient.setQueryData<TicketList>(ticketKeys.list(site.id), (current) => syncList(current, ticket));
    void navigate(`/tickets/${site.id}/${ticket.id}`);
  };

  if (loaded && !site) {
    // 当前站点没开工单（导航里本来不显示工单，从别处跳过来时说明一下）。
    return (
      <div className="space-y-6">
        <PageHeader title={t("tickets.title")} subtitle={t("tickets.subtitle")} />
        <Card className="flex flex-col items-center px-8 py-12 text-center">
          <LifeBuoy className="h-10 w-10 text-muted-foreground" aria-hidden />
          <h2 className="mt-4 text-lg font-medium">{current ? t("tickets.notEnabledTitle", { site: siteName(current) }) : t("tickets.noSitesTitle")}</h2>
          <p className="mt-2 max-w-md text-sm text-muted-foreground">{t("tickets.noSitesDesc")}</p>
          <Link to="/sites" className={cn(buttonVariants(), "mt-6")}>
            {current ? t("switcher.manage") : t("tickets.goSites")}
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

      {creating && site && allowed && (
        <NewTicketDialog site={site} onClose={closeNew} onCreated={onCreated} initialTitle={draft?.title} initialBody={draft?.body} />
      )}
    </div>
  );
}
