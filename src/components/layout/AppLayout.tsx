import { useEffect } from "react";
import { NavLink, Outlet } from "react-router";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { Globe, House, Info, LifeBuoy, Settings, Wrench, type LucideIcon } from "lucide-react";
import { applyBrandColor } from "../../brand";
import { useTicketFocusRefresh, useTicketLists, useTicketSites } from "../../lib/tickets";
import { useActiveBrand } from "../../lib/useActiveBrand";
import { BrandMark } from "./BrandMark";
import { LinkEvents } from "./LinkEvents";
import { TrayEvents } from "./TrayEvents";
import { MinVersionBanner } from "./MinVersionBanner";
import { UpdateBanner } from "./UpdateBanner";
import { getAppInfo } from "../../lib/api";
import { cn } from "../../lib/cn";
import { inDesktop } from "../../lib/tauri";

interface NavItem {
  to: string;
  label: "home" | "sites" | "tools" | "tickets" | "settings";
  icon: LucideIcon;
  end?: boolean;
}

const navItems: NavItem[] = [
  { to: "/", label: "home", icon: House, end: true },
  { to: "/sites", label: "sites", icon: Globe },
  { to: "/tools", label: "tools", icon: Wrench },
  // 接入的站点里有开了工单的才显示。
  { to: "/tickets", label: "tickets", icon: LifeBuoy },
  { to: "/settings", label: "settings", icon: Settings },
];

export function AppLayout() {
  const { t } = useTranslation();
  // 用专属链接接入的站点，换成站长的品牌。
  const brand = useActiveBrand();
  const { data: info } = useQuery({ queryKey: ["app-info"], queryFn: getAppInfo });
  // 工单：各站点有新回复的工单数加起来显示在导航上，每 5 分钟和窗口回到前台时刷新。
  const { sites: ticketSites } = useTicketSites();
  const ticketLists = useTicketLists(ticketSites);
  const unread = ticketLists.reduce((sum, list) => sum + (list.data?.unread ?? 0), 0);
  useTicketFocusRefresh();
  const items = navItems.filter((item) => item.label !== "tickets" || ticketSites.length > 0);

  useEffect(() => applyBrandColor(brand.color), [brand.color]);

  return (
    <div className="flex h-full">
      <aside className="flex w-56 shrink-0 flex-col border-r border-border bg-card">
        <div className="flex h-16 items-center gap-2.5 px-5">
          <BrandMark brand={brand} />
          <span className="truncate text-base font-semibold">{brand.name}</span>
        </div>
        <nav className="flex-1 space-y-1 px-3 py-2">
          {items.map(({ to, label, icon: Icon, end }) => (
            <NavLink
              key={to}
              to={to}
              end={end}
              className={({ isActive }) =>
                cn(
                  "flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand",
                  isActive ? "bg-brand font-medium text-brand-foreground" : "text-muted-foreground hover:bg-muted hover:text-foreground",
                )
              }
            >
              <Icon className="h-4 w-4" aria-hidden />
              {t(`nav.${label}`)}
              {label === "tickets" && unread > 0 && (
                <>
                  {/* 文字用背景色：浅色模式是白字，深色模式（红色变浅）是深色字，都看得清。 */}
                  <span className="ml-auto min-w-5 rounded-full bg-danger px-1.5 text-center text-[11px] font-semibold leading-5 text-background" aria-hidden>
                    {unread > 99 ? "99+" : unread}
                  </span>
                  <span className="sr-only">{t("tickets.unreadCount", { count: unread })}</span>
                </>
              )}
            </NavLink>
          ))}
        </nav>
        {info && <p className="px-5 py-4 text-xs text-muted-foreground">v{info.version}</p>}
      </aside>

      <main className="min-w-0 flex-1 overflow-y-auto">
        {!inDesktop && (
          <div className="flex items-center gap-2 border-b border-border bg-muted px-8 py-2 text-xs text-muted-foreground" role="status">
            <Info className="h-3.5 w-3.5 shrink-0" aria-hidden />
            {t("common.previewMode")}
          </div>
        )}
        <MinVersionBanner />
        <UpdateBanner />
        <TrayEvents />
        <LinkEvents />
        <div className="mx-auto max-w-4xl px-8 py-8">
          <Outlet />
        </div>
      </main>
    </div>
  );
}
