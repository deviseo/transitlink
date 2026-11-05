import { useEffect } from "react";
import { NavLink, Outlet } from "react-router";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { House, Info, KeyRound, LifeBuoy, Settings, Wrench, type LucideIcon } from "lucide-react";
import { applyBrandColor } from "../../brand";
import { useCurrentSite } from "../../lib/currentSite";
import { useTicketFocusRefresh, useTicketUnread } from "../../lib/tickets";
import { useActiveBrand } from "../../lib/useActiveBrand";
import { LinkEvents } from "./LinkEvents";
import { SiteSwitchProvider } from "./SiteSwitch";
import { SiteSwitcher } from "./SiteSwitcher";
import { TrayEvents } from "./TrayEvents";
import { MinVersionBanner } from "./MinVersionBanner";
import { UpdateBanner } from "./UpdateBanner";
import { getAppInfo } from "../../lib/api";
import { cn } from "../../lib/cn";
import { inDesktop } from "../../lib/tauri";

interface NavItem {
  to: string;
  label: "home" | "keys" | "tools" | "tickets" | "settings";
  icon: LucideIcon;
  end?: boolean;
}

/** 当前站点的功能（左上角切换站点，下面的页面都跟着换）。设置是全局的，单独放在底部。 */
const siteItems: NavItem[] = [
  { to: "/", label: "home", icon: House, end: true },
  { to: "/keys", label: "keys", icon: KeyRound },
  { to: "/tools", label: "tools", icon: Wrench },
  // 当前站点开了工单才显示。
  { to: "/tickets", label: "tickets", icon: LifeBuoy },
];
const settingsItem: NavItem = { to: "/settings", label: "settings", icon: Settings };

function NavItemLink({ item, badge = 0 }: { item: NavItem; badge?: number }) {
  const { t } = useTranslation();
  const { to, label, icon: Icon, end } = item;
  return (
    <NavLink
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
      {badge > 0 && (
        <>
          {/* 文字用背景色：浅色模式是白字，深色模式（红色变浅）是深色字，都看得清。 */}
          <span className="ml-auto min-w-5 rounded-full bg-danger px-1.5 text-center text-[11px] font-semibold leading-5 text-background" aria-hidden>
            {badge > 99 ? "99+" : badge}
          </span>
          <span className="sr-only">{t("tickets.unreadCount", { count: badge })}</span>
        </>
      )}
    </NavLink>
  );
}

export function AppLayout() {
  const { t } = useTranslation();
  // 当前站点是用专属链接接入的，换成站长的品牌。
  const brand = useActiveBrand();
  const { site } = useCurrentSite();
  const { data: info } = useQuery({ queryKey: ["app-info"], queryFn: getAppInfo });
  // 工单：各站点有新回复的工单数（每 5 分钟和窗口回到前台时刷新）。导航上显示当前站点的，切换器上提示其他站点的。
  const unread = useTicketUnread();
  useTicketFocusRefresh();
  const items = siteItems.filter((item) => {
    if (item.label === "keys") return site !== null;
    if (item.label === "tickets") return site?.config?.tickets?.enabled === true;
    return true;
  });

  useEffect(() => applyBrandColor(brand.color), [brand.color]);

  return (
    <SiteSwitchProvider>
      <div className="flex h-full">
        <aside className="flex w-56 shrink-0 flex-col border-r border-border bg-card">
          <SiteSwitcher unread={unread} />
          <nav className="flex-1 space-y-1 px-3 py-2">
            {items.map((item) => (
              <NavItemLink key={item.to} item={item} badge={item.label === "tickets" && site ? (unread.get(site.id) ?? 0) : 0} />
            ))}
          </nav>
          <div className="space-y-1 border-t border-border px-3 py-2">
            <NavItemLink item={settingsItem} />
          </div>
          {info && <p className="px-5 pb-4 pt-1 text-xs text-muted-foreground">v{info.version}</p>}
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
    </SiteSwitchProvider>
  );
}
