import { createContext, useContext, useState, type ReactNode } from "react";
import { useLocation, useNavigate } from "react-router";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { LoaderCircle } from "lucide-react";
import { Button } from "../ui/button";
import { Dialog } from "../ui/dialog";
import { detectTools, setCurrentSite, switchTools, type SiteView, type SwitchReport, type ToolKind } from "../../lib/api";
import { settingsKey, siteName, useCurrentSite } from "../../lib/currentSite";
import { errorText } from "../../lib/errors";

interface SiteSwitch {
  /** 切换到这个站点：Claude Code / Codex 在用别的站点时，先问用户要不要一起切。 */
  switchTo: (site: SiteView) => void;
}

const SiteSwitchContext = createContext<SiteSwitch>({ switchTo: () => undefined });

export const useSiteSwitch = () => useContext(SiteSwitchContext);

interface Pending {
  site: SiteView;
  /** 在用别的站点的工具，`from` 是那个站点的名字。 */
  tools: { kind: ToolKind; from: string }[];
}

/**
 * 切换当前站点。工具都已经在用这个站点（或者还没配置过工具）时直接切；否则弹窗让用户勾选要一起切的工具，
 * 不勾选的继续用原来的站点。切换后，在工单页的回到新站点的工单列表，新站点没开工单的回到概览。
 */
export function SiteSwitchProvider({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const tools = useQuery({ queryKey: ["tools"], queryFn: detectTools });
  const { sites } = useCurrentSite();
  const [pending, setPending] = useState<Pending | null>(null);
  const [chosen, setChosen] = useState<ToolKind[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [report, setReport] = useState<SwitchReport | null>(null);

  const nameOf = (id: string, fallback: string) => {
    const site = sites.find((item) => item.id === id);
    return site ? siteName(site) : fallback;
  };

  const apply = async (site: SiteView, kinds: ToolKind[]): Promise<SwitchReport | null> => {
    queryClient.setQueryData(settingsKey, await setCurrentSite(site.id));
    let result: SwitchReport | null = null;
    if (kinds.length > 0) {
      result = await switchTools(site.id, kinds);
      void queryClient.invalidateQueries({ queryKey: ["tools"] });
    }
    if (pathname.startsWith("/tickets")) void navigate(site.config?.tickets?.enabled ? "/tickets" : "/");
    return result;
  };

  const switchTo = (site: SiteView) => {
    const elsewhere = (tools.data ?? []).flatMap((tool) =>
      tool.applied && tool.applied.siteId !== site.id ? [{ kind: tool.kind, from: nameOf(tool.applied.siteId, tool.applied.siteName) }] : [],
    );
    setError(null);
    setReport(null);
    if (elsewhere.length === 0) {
      // 不用问，直接切；出错时才弹窗说明。
      void apply(site, []).catch((caught: unknown) => {
        setPending({ site, tools: [] });
        setError(caught);
      });
      return;
    }
    setChosen(elsewhere.map((tool) => tool.kind));
    setPending({ site, tools: elsewhere });
  };

  const confirm = async () => {
    if (!pending) return;
    setBusy(true);
    setError(null);
    try {
      const result = await apply(pending.site, chosen);
      if (result && result.failures.length > 0) setReport(result);
      else setPending(null);
    } catch (caught) {
      setError(caught);
    } finally {
      setBusy(false);
    }
  };

  const close = () => {
    if (!busy) setPending(null);
  };

  const target = pending ? siteName(pending.site) : "";
  return (
    <SiteSwitchContext.Provider value={{ switchTo }}>
      {children}
      <Dialog open={pending !== null} title={t("switchSite.title", { site: target })} onClose={close} closeLabel={t("common.close")}>
        {pending && report ? (
          <div className="space-y-4">
            {report.switched.length > 0 && (
              <p className="text-sm">{t("switchSite.switched", { tools: report.switched.map((kind) => t(`tools.${kind}`)).join(t("common.separator")) })}</p>
            )}
            <div role="alert" className="space-y-1 rounded-lg bg-danger/10 p-3 text-sm text-danger">
              <p>{t("switchSite.failed")}</p>
              <ul>
                {report.failures.map((failure) => (
                  <li key={failure.kind}>{t("switchSite.failure", { tool: t(`tools.${failure.kind}`), error: errorText(t, failure.error) })}</li>
                ))}
              </ul>
            </div>
            <div className="flex justify-end">
              <Button onClick={() => setPending(null)}>{t("switchSite.ok")}</Button>
            </div>
          </div>
        ) : (
          pending && (
            <div className="space-y-4">
              {pending.tools.length > 0 && (
                <>
                  <p className="text-sm">{t("switchSite.question")}</p>
                  <ul className="space-y-2">
                    {pending.tools.map((tool) => {
                      const id = `switch-${tool.kind}`;
                      return (
                        <li key={tool.kind}>
                          <label htmlFor={id} className="flex cursor-pointer items-center gap-3 rounded-lg border border-border p-3 hover:bg-muted/60">
                            <input
                              id={id}
                              type="checkbox"
                              checked={chosen.includes(tool.kind)}
                              onChange={(event) =>
                                setChosen((current) => (event.target.checked ? [...current, tool.kind] : current.filter((kind) => kind !== tool.kind)))
                              }
                              disabled={busy}
                              className="h-4 w-4 shrink-0 accent-brand"
                            />
                            <span className="min-w-0 flex-1">
                              <span className="block font-medium">{t(`tools.${tool.kind}`)}</span>
                              <span className="block truncate text-xs text-muted-foreground">{t("switchSite.using", { site: tool.from })}</span>
                            </span>
                          </label>
                        </li>
                      );
                    })}
                  </ul>
                  <p className="text-xs text-muted-foreground">{t("switchSite.keepHint", { site: target })}</p>
                </>
              )}
              {error != null && (
                <p role="alert" className="text-sm text-danger">
                  {errorText(t, error)}
                </p>
              )}
              <div className="flex justify-end gap-2">
                <Button variant="secondary" onClick={close} disabled={busy}>
                  {t("common.cancel")}
                </Button>
                <Button onClick={() => void confirm()} disabled={busy}>
                  {busy && <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />}
                  {t("switchSite.confirm")}
                </Button>
              </div>
            </div>
          )
        )}
      </Dialog>
    </SiteSwitchContext.Provider>
  );
}
