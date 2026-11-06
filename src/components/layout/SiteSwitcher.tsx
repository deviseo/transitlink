import { useEffect, useId, useRef, useState } from "react";
import { useNavigate } from "react-router";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, ChevronsUpDown, Plus, Settings2, Terminal } from "lucide-react";
import { AddSiteDialog } from "../sites/AddSiteDialog";
import { SiteAvatar } from "../sites/SiteAvatar";
import { SetupDialog } from "../tools/SetupDialog";
import { Button } from "../ui/button";
import { BrandMark } from "./BrandMark";
import { useSiteSwitch } from "./SiteSwitch";
import { detectTools, type SiteView } from "../../lib/api";
import { siteName, useCurrentSite } from "../../lib/currentSite";
import { useActiveBrand } from "../../lib/useActiveBrand";

const itemClass =
  "flex w-full items-center gap-2.5 rounded-lg px-2 py-2 text-left text-sm transition-colors hover:bg-muted focus-visible:bg-muted focus-visible:outline-none";

/** 站点的小图标：用专属链接接入的用站长的 logo。 */
function SiteIcon({ site, className }: { site: SiteView; className?: string }) {
  return <SiteAvatar name={siteName(site)} logoUrl={site.config?.brand.logoUrl || site.logoUrl} className={className} />;
}

/**
 * 左上角的当前站点：概览、密钥、工单都看这个站点。点开可以切换站点、接入新站点、管理站点；
 * 其他站点有新的工单回复时，图标上有红点，菜单里显示条数。
 */
export function SiteSwitcher({ unread }: { unread: Map<string, number> }) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { site, sites, loaded } = useCurrentSite();
  const brand = useActiveBrand();
  const { switchTo } = useSiteSwitch();
  const tools = useQuery({ queryKey: ["tools"], queryFn: detectTools });
  const [open, setOpen] = useState(false);
  const [adding, setAdding] = useState(false);
  const [setupSite, setSetupSite] = useState<string | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const menuId = useId();

  // 点外面或按 Esc 收起；打开时焦点放到当前站点上，上下键在菜单里移动。
  useEffect(() => {
    if (!open) return;
    const items = () => Array.from(menu.current?.querySelectorAll<HTMLElement>("[role^=menuitem]") ?? []);
    (items().find((item) => item.getAttribute("aria-checked") === "true") ?? items()[0])?.focus();
    const onPointer = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
        root.current?.querySelector<HTMLElement>("[aria-haspopup]")?.focus();
      } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        const list = items();
        const index = list.indexOf(document.activeElement as HTMLElement);
        list[(index + (event.key === "ArrowDown" ? 1 : list.length - 1)) % list.length]?.focus();
      }
    };
    document.addEventListener("pointerdown", onPointer);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onPointer);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const dialogs = (
    <>
      <AddSiteDialog
        open={adding}
        onClose={() => setAdding(false)}
        onAdded={(added, viaLink) => {
          setAdding(false);
          void queryClient.invalidateQueries({ queryKey: ["sites"] });
          // 新登录的站点接着一键配置（和点专属链接接入一样）。
          if (viaLink) setSetupSite(added.id);
        }}
      />
      <SetupDialog open={setupSite !== null} initialSiteId={setupSite ?? undefined} onClose={() => setSetupSite(null)} />
    </>
  );

  if (!loaded) return <div className="h-[4.5rem]" aria-hidden />;

  if (!site) {
    // 还没有站点：显示 TransitLink 和「接入站点」。
    return (
      <div className="space-y-3 px-4 py-4">
        <div className="flex items-center gap-2.5 px-1">
          <BrandMark brand={brand} />
          <span className="truncate text-base font-semibold">{brand.name}</span>
        </div>
        <Button size="sm" className="w-full" onClick={() => setAdding(true)}>
          <Plus className="h-4 w-4" aria-hidden />
          {t("switcher.connect")}
        </Button>
        {dialogs}
      </div>
    );
  }

  const inUse = new Set((tools.data ?? []).map((tool) => tool.applied?.siteId).filter(Boolean));
  const otherUnread = sites.some((item) => item.id !== site.id && (unread.get(item.id) ?? 0) > 0);
  return (
    <div ref={root} className="relative px-3 py-3">
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        aria-label={t("switcher.current", { site: siteName(site), account: site.account })}
        onClick={() => setOpen((value) => !value)}
        className="flex w-full items-center gap-2.5 rounded-xl px-2 py-2 text-left transition hover:bg-surface/70 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand/60"
      >
        <span className="relative shrink-0">
          {brand.site ? <BrandMark brand={brand} /> : <SiteIcon site={site} className="h-8 w-8" />}
          {otherUnread && <span className="absolute -right-1 -top-1 h-3 w-3 rounded-full border-2 border-background bg-danger" aria-hidden />}
        </span>
        <span className="min-w-0 flex-1">
          <span className="block truncate text-sm font-semibold">{siteName(site)}</span>
          <span className="block truncate text-xs text-muted-foreground" title={site.account}>
            {site.account}
          </span>
        </span>
        <ChevronsUpDown className="h-4 w-4 shrink-0 text-muted-foreground" aria-hidden />
      </button>
      {otherUnread && <span className="sr-only">{t("switcher.otherUnread")}</span>}

      {open && (
        <div
          ref={menu}
          id={menuId}
          role="menu"
          aria-label={t("switcher.menu")}
          className="absolute left-3 top-full z-40 -mt-1 w-72 rounded-xl border border-border bg-card p-1.5 shadow-[0_12px_32px_-8px_rgba(16,24,40,0.25)]"
        >
          <p className="px-2 pb-1 pt-1.5 text-xs font-medium text-muted-foreground">{t("switcher.menu")}</p>
          {sites.map((item) => {
            const current = item.id === site.id;
            const count = unread.get(item.id) ?? 0;
            return (
              <button
                key={item.id}
                type="button"
                role="menuitemradio"
                aria-checked={current}
                onClick={() => {
                  setOpen(false);
                  if (!current) switchTo(item);
                }}
                className={itemClass}
              >
                <SiteIcon site={item} className="h-7 w-7" />
                <span className="min-w-0 flex-1">
                  <span className="block truncate font-medium">{siteName(item)}</span>
                  <span className="block truncate text-xs text-muted-foreground">{item.account}</span>
                </span>
                {inUse.has(item.id) && (
                  <span title={t("switcher.toolsHere")} className="shrink-0 text-muted-foreground">
                    <Terminal className="h-3.5 w-3.5" aria-hidden />
                    <span className="sr-only">{t("switcher.toolsHere")}</span>
                  </span>
                )}
                {count > 0 && (
                  <span className="shrink-0 rounded-full bg-danger px-1.5 text-[11px] font-semibold leading-5 text-background">
                    <span aria-hidden>{count > 99 ? "99+" : count}</span>
                    <span className="sr-only">{t("tickets.unreadCount", { count })}</span>
                  </span>
                )}
                <Check className={`h-4 w-4 shrink-0 text-brand ${current ? "" : "invisible"}`} aria-hidden />
              </button>
            );
          })}
          <div className="my-1 border-t border-border" role="separator" />
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setOpen(false);
              setAdding(true);
            }}
            className={itemClass}
          >
            <Plus className="h-4 w-4 text-muted-foreground" aria-hidden />
            {t("switcher.add")}
          </button>
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setOpen(false);
              void navigate("/sites");
            }}
            className={itemClass}
          >
            <Settings2 className="h-4 w-4 text-muted-foreground" aria-hidden />
            {t("switcher.manage")}
          </button>
        </div>
      )}
      {dialogs}
    </div>
  );
}
