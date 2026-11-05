import { useEffect, useState } from "react";
import { Link } from "react-router";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { CircleAlert, CircleCheck, CircleX, LoaderCircle } from "lucide-react";
import { Button, buttonVariants } from "../ui/button";
import { Dialog } from "../ui/dialog";
import { applyTool, detectTools, listSites, setupPlan, type ApplyOutcome, type GroupSummary, type ToolKind } from "../../lib/api";
import { errorText } from "../../lib/errors";
import { siteName } from "../../lib/currentSite";
import { ALL_TOOLS, familyKey, testText, toolCommands } from "../../lib/tools";

type Run = { status: "running" } | { status: "done"; outcome: ApplyOutcome } | { status: "failed"; error: unknown };
type Phase = "choose" | "running" | "done";


const selectClass =
  "h-9 min-w-0 rounded-lg border border-border bg-card px-2 text-sm text-foreground outline-none focus:border-brand focus:ring-1 focus:ring-brand disabled:opacity-50";

function groupLabel(group: GroupSummary) {
  const ratio = group.ratio != null ? ` ×${group.ratio}` : "";
  const description = group.description && group.description !== group.name ? ` · ${group.description}` : "";
  return `${group.name}${ratio}${description}`;
}

/** 一键配置：选站点和分组，依次给每个工具建专用 Key、测试连通、写入配置。`only` 时只配置这一个工具。 */
export function SetupDialog({ open, onClose, only, initialSiteId }: { open: boolean; onClose: () => void; only?: ToolKind; initialSiteId?: string }) {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const sites = useQuery({ queryKey: ["sites"], queryFn: listSites, enabled: open });
  const tools = useQuery({ queryKey: ["tools"], queryFn: detectTools, enabled: open });
  const [siteId, setSiteId] = useState("");
  const [chosen, setChosen] = useState<Partial<Record<ToolKind, boolean>>>({});
  const [groups, setGroups] = useState<Partial<Record<ToolKind, string>>>({});
  const [runs, setRuns] = useState<Partial<Record<ToolKind, Run>>>({});
  const [phase, setPhase] = useState<Phase>("choose");
  const visible = only ? [only] : ALL_TOOLS;

  // 每次打开都从选择开始。
  useEffect(() => {
    if (!open) return;
    setPhase("choose");
    setRuns({});
    setGroups({});
    // 默认勾选装好了的工具；一个都没装时勾选 Claude Code 和 Codex（配置好后装上就能用）。
    const installed = (tools.data ?? []).filter((tool) => tool.installed).map((tool) => tool.kind);
    const defaults: ToolKind[] = only ? [only] : installed.length > 0 ? installed : ["claudeCode", "codex"];
    setChosen(Object.fromEntries(ALL_TOOLS.map((kind) => [kind, defaults.includes(kind)])));
    // 从托盘点了某个站点：直接选中它。
    if (initialSiteId) setSiteId(initialSiteId);
    // tools.data 只在打开时用一次，不放进依赖：重新检测工具时不要重置用户的勾选。
  }, [open, only, initialSiteId]);

  // 默认站点：托盘点的那个，或者工具已经配置到的那个，否则第一个。
  useEffect(() => {
    const list = sites.data ?? [];
    if (!open || list.length === 0 || list.some((site) => site.id === siteId)) return;
    const configured = tools.data?.find((tool) => (!only || tool.kind === only) && tool.applied)?.applied?.siteId;
    const preferred = [initialSiteId, configured].find((id) => id && list.some((site) => site.id === id));
    setSiteId(preferred ?? list[0].id);
  }, [open, siteId, sites.data, tools.data, only, initialSiteId]);

  const plan = useQuery({ queryKey: ["setup-plan", siteId], queryFn: () => setupPlan(siteId), enabled: open && siteId !== "" });
  const money = new Intl.NumberFormat(i18n.language, { style: "currency", currency: "USD", maximumFractionDigits: 2 });
  const groupFor = (kind: ToolKind) => groups[kind] ?? plan.data?.tools.find((tool) => tool.kind === kind)?.recommendedGroup ?? "";
  const picked = visible.filter((kind) => chosen[kind]);

  const start = async () => {
    setPhase("running");
    setRuns(Object.fromEntries(picked.map((kind) => [kind, { status: "running" }])));
    // 一个一个来：每个工具都要建 Key、测试、写配置。
    for (const kind of picked) {
      try {
        // 站长推荐了模型就用它（所选分组没有这个模型时，桌面端会自动换一个）。
        const model = plan.data?.tools.find((tool) => tool.kind === kind)?.recommendedModel ?? null;
        const outcome = await applyTool(kind, siteId, groupFor(kind) || null, model);
        setRuns((current) => ({ ...current, [kind]: { status: "done", outcome } }));
      } catch (error) {
        setRuns((current) => ({ ...current, [kind]: { status: "failed", error } }));
      }
    }
    setPhase("done");
    void queryClient.invalidateQueries({ queryKey: ["tools"] });
    void queryClient.invalidateQueries({ queryKey: ["site-keys", siteId] });
  };

  // 配置进行中不能关掉窗口（Esc 或点遮罩），免得以为中断了。
  const close = () => {
    if (phase !== "running") onClose();
  };

  const results = picked.map((kind) => [kind, runs[kind]] as const).filter(([, run]) => run);
  const succeeded = results.filter(([, run]) => run?.status === "done").map(([kind]) => kind);
  const allOk = phase === "done" && succeeded.length === results.length;

  return (
    <Dialog open={open} title={t("setup.title")} onClose={close} closeLabel={t("common.close")} wide>
      {sites.data && sites.data.length === 0 ? (
        <div className="space-y-4">
          <p className="text-sm text-muted-foreground">{t("setup.noSites")}</p>
          <div className="flex justify-end">
            <Link to="/sites" onClick={onClose} className={buttonVariants({ size: "sm" })}>
              {t("setup.goSites")}
            </Link>
          </div>
        </div>
      ) : phase === "choose" ? (
        <div className="space-y-4">
          <div className="flex items-center gap-3">
            <label htmlFor="setup-site" className="w-12 shrink-0 text-sm font-medium">
              {t("setup.site")}
            </label>
            <select id="setup-site" value={siteId} onChange={(event) => { setSiteId(event.target.value); setGroups({}); }} className={`${selectClass} flex-1`}>
              {(sites.data ?? []).map((site) => (
                <option key={site.id} value={site.id}>
                  {siteName(site)} · {site.account}
                </option>
              ))}
            </select>
          </div>

          {plan.isLoading && <LoaderCircle className="h-4 w-4 animate-spin text-muted-foreground" aria-hidden />}
          {plan.isError && <p className="text-sm text-danger">{errorText(t, plan.error)}</p>}
          {plan.data && (
            <>
              {plan.data.balanceUsd != null &&
                (plan.data.balanceUsd <= 0 ? (
                  <p className="flex items-center gap-1.5 text-sm text-warning">
                    <CircleAlert className="h-4 w-4 shrink-0" aria-hidden />
                    {t("setup.lowBalance")}
                  </p>
                ) : (
                  <p className="text-xs text-muted-foreground">{t("setup.balance", { amount: money.format(plan.data.balanceUsd) })}</p>
                ))}
              <ul className="space-y-2">
                {visible.map((kind) => {
                  const toolPlan = plan.data.tools.find((tool) => tool.kind === kind);
                  const installed = tools.data?.find((tool) => tool.kind === kind)?.installed ?? true;
                  const id = `setup-${kind}`;
                  return (
                    <li key={kind} className="rounded-lg border border-border p-3">
                      <div className="flex items-center gap-3">
                        {!only && (
                          <input
                            id={id}
                            type="checkbox"
                            checked={chosen[kind] ?? false}
                            onChange={(event) => setChosen((current) => ({ ...current, [kind]: event.target.checked }))}
                            className="h-4 w-4 shrink-0 accent-brand"
                          />
                        )}
                        <label htmlFor={only ? `${id}-group` : id} className="min-w-0 flex-1 font-medium">
                          {t(`tools.${kind}`)}
                        </label>
                        <label htmlFor={`${id}-group`} className="sr-only">
                          {t("setup.group")}
                        </label>
                        <select
                          id={`${id}-group`}
                          value={groupFor(kind)}
                          onChange={(event) => setGroups((current) => ({ ...current, [kind]: event.target.value }))}
                          disabled={!chosen[kind]}
                          className={`${selectClass} w-64`}
                        >
                          {plan.data.groups.length === 0 && <option value="">{t("setup.defaultGroup")}</option>}
                          {plan.data.groups.map((group) => (
                            <option key={group.id} value={group.id}>
                              {groupLabel(group)}
                              {toolPlan?.matched && toolPlan.recommendedGroup === group.id ? ` · ${t("setup.recommendedTag")}` : ""}
                            </option>
                          ))}
                        </select>
                      </div>
                      {chosen[kind] && toolPlan && !toolPlan.matched && plan.data.groups.length > 0 && groups[kind] === undefined && (
                        <p className="mt-2 flex items-start gap-1.5 text-xs text-warning">
                          <CircleAlert className="mt-px h-3.5 w-3.5 shrink-0" aria-hidden />
                          {t("setup.groupNotMatched", {
                            tool: t(`tools.${kind}`),
                            family: t(familyKey[kind]),
                          })}
                        </p>
                      )}
                      {chosen[kind] && toolPlan?.recommendedModel && (
                        <p className="mt-2 text-xs text-muted-foreground">{t("setup.recommendedModel", { model: toolPlan.recommendedModel })}</p>
                      )}
                      {chosen[kind] && !installed && <p className="mt-2 text-xs text-muted-foreground">{t("setup.notInstalled")}</p>}
                    </li>
                  );
                })}
              </ul>
            </>
          )}

          <p className="text-xs leading-5 text-muted-foreground">{t("setup.explain")}</p>
          <div className="flex items-center justify-end gap-2">
            {picked.length === 0 && <span className="mr-auto text-xs text-muted-foreground">{t("setup.pickOne")}</span>}
            <Button variant="ghost" onClick={close}>
              {t("common.cancel")}
            </Button>
            <Button onClick={() => void start()} disabled={!plan.data || picked.length === 0}>
              {t("setup.start")}
            </Button>
          </div>
        </div>
      ) : (
        <div className="space-y-4">
          <ul className="space-y-2">
            {results.map(([kind, run]) => (
              <li key={kind} className="flex items-start gap-3 rounded-lg border border-border p-3">
                {run?.status === "running" && <LoaderCircle className="mt-0.5 h-5 w-5 shrink-0 animate-spin text-muted-foreground" aria-hidden />}
                {run?.status === "done" && <CircleCheck className="mt-0.5 h-5 w-5 shrink-0 text-success" aria-hidden />}
                {run?.status === "failed" && <CircleX className="mt-0.5 h-5 w-5 shrink-0 text-danger" aria-hidden />}
                <div className="min-w-0 flex-1">
                  <p className="font-medium">{t(`tools.${kind}`)}</p>
                  {run?.status === "running" && <p className="text-sm text-muted-foreground">{t("setup.running")}</p>}
                  {run?.status === "done" && (
                    <>
                      <p className="text-sm">
                        {run.outcome.model
                          ? t("setup.configuredModel", { group: run.outcome.group ?? t("setup.defaultGroup"), model: run.outcome.model })
                          : t("setup.configured", { group: run.outcome.group ?? t("setup.defaultGroup") })}
                      </p>
                      <p className="text-xs text-muted-foreground">
                        {t(run.outcome.keyCreated ? "setup.keyCreated" : "setup.keyReused", { key: run.outcome.keyName })}
                        {t("common.separator")}
                        {testText(t, run.outcome.test)}
                      </p>
                      {kind === "geminiCli" && <p className="mt-1 text-xs text-warning">{t("tools.geminiTrust")}</p>}
                    </>
                  )}
                  {run?.status === "failed" && <p className="text-sm text-danger">{errorText(t, run.error)}</p>}
                </div>
              </li>
            ))}
          </ul>

          {phase === "done" &&
            (allOk ? (
              <p className="rounded-lg bg-success/10 p-3 text-sm text-success">
                {t("setup.allDone", { commands: succeeded.map((kind) => toolCommands[kind]).join(t("common.separator")) })}
              </p>
            ) : (
              <p className="text-sm text-muted-foreground">{t("setup.someFailed")}</p>
            ))}
          <div className="flex justify-end gap-2">
            {phase === "done" && !allOk && (
              <Button
                variant="secondary"
                onClick={() => {
                  // 配置成功的不用再来一遍，只改没成功的。
                  setChosen((current) => ({ ...current, ...Object.fromEntries(succeeded.map((kind) => [kind, false])) }));
                  setPhase("choose");
                }}
              >
                {t("setup.editAgain")}
              </Button>
            )}
            <Button onClick={close} disabled={phase === "running"}>
              {t("setup.finish")}
            </Button>
          </div>
        </div>
      )}
    </Dialog>
  );
}
