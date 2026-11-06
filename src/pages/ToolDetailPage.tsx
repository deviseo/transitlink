import { useEffect, useState, type ReactNode } from "react";
import { Link, useParams } from "react-router";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowRightLeft, ArrowUpCircle, ChevronLeft, CircleAlert, CircleX, LoaderCircle, RotateCcw, Sparkles, Zap } from "lucide-react";
import { InstallPanel, UninstallPanel, UpgradePanel } from "../components/tools/InstallPanel";
import { SetupDialog } from "../components/tools/SetupDialog";
import { ToolIcon } from "../components/tools/ToolIcon";
import { UninstallConfirm } from "../components/tools/UninstallConfirm";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { SectionCard } from "../components/ui/card";
import {
  detectTools,
  getAppInfo,
  restoreTool,
  setToolModel,
  switchTools,
  testTool,
  toolModels,
  type ToolLatest,
  type ToolOverview,
} from "../lib/api";
import { siteName, useCurrentSite } from "../lib/currentSite";
import { errorText } from "../lib/errors";
import { useToolInstall, useToolVersions } from "../lib/install";
import { testText, toolCommands } from "../lib/tools";
import { toolStatus } from "../lib/toolStatus";

/** 一个工具的详情：上面是和站点的连接（配置、测试、换模型、还原），下面是程序本身（安装、升级、卸载）。 */
export function ToolDetailPage() {
  const { t } = useTranslation();
  const { kind } = useParams();
  const tools = useQuery({ queryKey: ["tools"], queryFn: detectTools });
  const versions = useToolVersions(tools.data);
  const app = useQuery({ queryKey: ["app-info"], queryFn: getAppInfo });
  const { site: current, sites } = useCurrentSite();
  const [setup, setSetup] = useState(false);
  const tool = tools.data?.find((item) => item.kind === kind);

  const back = (
    <Link to="/tools" className="inline-flex items-center gap-1 text-[13px] text-muted-foreground transition hover:text-foreground">
      <ChevronLeft className="h-4 w-4" aria-hidden />
      {t("tools.detail.back")}
    </Link>
  );
  if (tools.isLoading) return <LoaderCircle className="h-5 w-5 animate-spin text-muted-foreground" aria-hidden />;
  if (!tool) {
    return (
      <div className="space-y-4">
        {back}
        <p className="text-sm text-muted-foreground">{t("tools.detail.notFound")}</p>
      </div>
    );
  }
  const status = toolStatus(t, tool, current, sites);

  return (
    <div className="space-y-5">
      {back}
      <header className="flex flex-wrap items-center gap-4">
        <ToolIcon kind={tool.kind} size="lg" />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h1 className="text-[22px] font-semibold tracking-tight">{t(`tools.${tool.kind}`)}</h1>
            <Badge tone={status.tone}>{status.text}</Badge>
          </div>
          <p className="mt-0.5 text-[13px] text-muted-foreground">{t(`tools.${tool.kind}Desc`)}</p>
        </div>
        {tool.installed && (
          <div className="text-right">
            <p className="text-[11px] text-muted-foreground">{t("tools.detail.launch")}</p>
            <code className="font-mono text-[13px]">{toolCommands[tool.kind]}</code>
          </div>
        )}
      </header>
      {tool.configurable ? (
        <ConnectionCard tool={tool} hasSites={sites.length > 0} onSetup={() => setSetup(true)} />
      ) : (
        <SectionCard title={t("tools.detail.connection")}>
          <p className="text-[13px] leading-relaxed text-muted-foreground">{t("tools.detail.notConfigurable")}</p>
        </SectionCard>
      )}
      <ProgramCard tool={tool} latest={versions.data?.find((item) => item.kind === tool.kind)} windows={app.data?.platform === "windows"} />
      <SetupDialog open={setup} only={tool.kind} initialSiteId={current?.id} onClose={() => setSetup(false)} />
    </div>
  );
}

/** 详情里的一行：左边标签，右边内容。 */
function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[5.5rem_1fr] items-start gap-3 py-2 text-[13px] first:pt-0 last:pb-0">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="min-w-0">{children}</dd>
    </div>
  );
}

/** 和站点的连接：一键配置、测试连接、换模型、改用当前站点、还原。 */
function ConnectionCard({ tool, hasSites, onSetup }: { tool: ToolOverview; hasSites: boolean; onSetup: () => void }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [confirming, setConfirming] = useState(false);
  const refresh = () => void queryClient.invalidateQueries({ queryKey: ["tools"] });
  const test = useMutation({ mutationFn: () => testTool(tool.kind) });
  const restore = useMutation({
    mutationFn: () => restoreTool(tool.kind),
    onSuccess: () => {
      setConfirming(false);
      test.reset();
      refresh();
    },
  });
  const ours = tool.state === "configured" || tool.state === "changed";
  const { site: current, sites } = useCurrentSite();
  const appliedSite = sites.find((site) => site.id === tool.applied?.siteId);
  const elsewhere = tool.state === "configured" && tool.applied && current && tool.applied.siteId !== current.id;
  const useCurrent = useMutation({
    mutationFn: async () => {
      if (!current) return;
      const report = await switchTools(current.id, [tool.kind]);
      if (report.failures.length > 0) throw report.failures[0].error;
    },
    onSuccess: () => test.reset(),
    onSettled: refresh,
  });

  const description =
    tool.state === "configured" && tool.applied
      ? t("tools.state.configured", { site: appliedSite ? siteName(appliedSite) : tool.applied.siteName })
      : tool.state === "notConfigured"
        ? t("tools.detail.notConnected")
        : undefined;

  return (
    <SectionCard
      title={t("tools.detail.connection")}
      description={description}
      action={
        tool.state !== "invalid" && (
          <Button size="sm" variant={ours ? "secondary" : "primary"} onClick={onSetup} disabled={!hasSites} title={hasSites ? undefined : t("tools.needSite")}>
            <Sparkles className="h-4 w-4" aria-hidden />
            {ours ? t("tools.reconfigure") : t("tools.configure")}
          </Button>
        )
      }
    >
      {tool.state === "invalid" && (
        <p className="flex items-start gap-1.5 rounded-lg bg-danger/10 p-3 text-[13px] text-danger">
          <CircleX className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
          {t("tools.state.invalid")}
        </p>
      )}
      {tool.state === "configured" && tool.applied && (
        <dl className="divide-y divide-border/70 rounded-xl bg-muted/50 px-4 py-3">
          <Row label={t("tools.key")}>{tool.applied.keyName}</Row>
          <Row label={t("tools.group")}>{tool.applied.group ?? t("setup.defaultGroup")}</Row>
          <ModelRow tool={tool} onChanged={refresh} />
        </dl>
      )}
      {tool.state === "configured" && tool.kind === "geminiCli" && <p className="mt-3 text-xs text-muted-foreground">{t("tools.geminiTrust")}</p>}
      {tool.state === "configured" && tool.kind === "deepseekHarness" && <p className="mt-3 text-xs text-muted-foreground">{t("tools.dshNote")}</p>}
      {tool.state === "configured" && tool.kind === "openclaw" && <p className="mt-3 text-xs text-muted-foreground">{t("tools.openclawNote")}</p>}
      {tool.state === "changed" && (
        <p className="flex items-start gap-1.5 text-[13px] text-warning">
          <CircleAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
          {t("tools.state.changed")}
        </p>
      )}
      {tool.state === "other" && (
        <p className="break-all text-[13px] text-muted-foreground">{t("tools.state.other", { target: tool.currentBaseUrl ?? tool.currentProvider ?? "" })}</p>
      )}
      {!hasSites && tool.state === "notConfigured" && <p className="text-[13px] text-muted-foreground">{t("tools.needSite")}</p>}

      {elsewhere && current && (
        <div className="mt-3 flex flex-wrap items-center justify-between gap-2 rounded-xl border border-warning/30 bg-warning/5 px-4 py-3">
          <p className="text-xs text-muted-foreground">{t("tools.notCurrent", { site: siteName(current) })}</p>
          <Button variant="secondary" size="sm" onClick={() => useCurrent.mutate()} disabled={useCurrent.isPending}>
            {useCurrent.isPending ? <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden /> : <ArrowRightLeft className="h-4 w-4" aria-hidden />}
            {t("tools.useCurrent", { site: siteName(current) })}
          </Button>
          {useCurrent.isError && <p className="w-full text-xs text-danger">{errorText(t, useCurrent.error)}</p>}
        </div>
      )}

      {test.data && <p className={`mt-3 text-[13px] ${test.data.status === "ok" ? "text-success" : "text-danger"}`}>{testText(t, test.data)}</p>}
      {test.isError && <p className="mt-3 text-[13px] text-danger">{errorText(t, test.error)}</p>}
      {restore.isError && <p className="mt-3 text-[13px] text-danger">{errorText(t, restore.error)}</p>}

      {confirming ? (
        <div className="mt-3 space-y-2 rounded-xl border border-border p-3">
          <p className="text-[13px] font-medium">{t("tools.restoreConfirm")}</p>
          <p className="text-xs text-muted-foreground">{t("tools.restoreHint")}</p>
          <div className="flex justify-end gap-2">
            <Button variant="ghost" size="sm" onClick={() => setConfirming(false)}>
              {t("common.cancel")}
            </Button>
            <Button size="sm" variant="danger" onClick={() => restore.mutate()} disabled={restore.isPending}>
              {restore.isPending && <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />}
              {t("tools.restore")}
            </Button>
          </div>
        </div>
      ) : (
        (tool.state === "configured" || ours) && (
          <div className="mt-3 flex flex-wrap items-center gap-2">
            {tool.state === "configured" && (
              <Button variant="secondary" size="sm" onClick={() => test.mutate()} disabled={test.isPending}>
                {test.isPending ? <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden /> : <Zap className="h-4 w-4" aria-hidden />}
                {t("tools.test")}
              </Button>
            )}
            {ours && (
              <Button variant="ghost" size="sm" onClick={() => setConfirming(true)}>
                <RotateCcw className="h-4 w-4" aria-hidden />
                {t("tools.restore")}
              </Button>
            )}
          </div>
        )
      )}
    </SectionCard>
  );
}

/** 当前模型；点「更换」从站点读取可选的模型。Claude Code 可以选「默认」，由它自己决定。 */
function ModelRow({ tool, onChanged }: { tool: ToolOverview; onChanged: () => void }) {
  const { t } = useTranslation();
  const [editing, setEditing] = useState(false);
  const models = useQuery({ queryKey: ["tool-models", tool.kind], queryFn: () => toolModels(tool.kind), enabled: editing });
  const save = useMutation({
    mutationFn: (model: string | null) => setToolModel(tool.kind, model),
    onSuccess: () => {
      setEditing(false);
      onChanged();
    },
  });
  const current = tool.applied?.model ?? null;

  return (
    <Row label={t("tools.model")}>
      <div className="flex min-w-0 items-center gap-2">
        {!editing ? (
          <>
            <span className="min-w-0 truncate">{current ?? t("tools.defaultModel")}</span>
            <button type="button" onClick={() => setEditing(true)} className="shrink-0 text-xs font-medium text-brand hover:underline">
              {t("tools.changeModel")}
            </button>
          </>
        ) : models.isLoading ? (
          <span className="flex items-center gap-1.5 text-muted-foreground">
            <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />
            {t("tools.loadingModels")}
          </span>
        ) : models.isError ? (
          <span className="text-danger">{errorText(t, models.error)}</span>
        ) : (
          <select
            aria-label={t("tools.model")}
            value={current ?? ""}
            onChange={(event) => save.mutate(event.target.value || null)}
            disabled={save.isPending}
            className="h-8 min-w-0 flex-1 rounded-lg border border-border bg-card px-2 text-[13px] outline-none focus:border-brand focus:ring-1 focus:ring-brand"
          >
            {tool.kind === "claudeCode" && <option value="">{t("tools.defaultModel")}</option>}
            {current && !(models.data ?? []).includes(current) && <option value={current}>{current}</option>}
            {(models.data ?? []).map((id) => (
              <option key={id} value={id}>
                {id}
              </option>
            ))}
          </select>
        )}
        {save.isError && <span className="text-xs text-danger">{errorText(t, save.error)}</span>}
      </div>
    </Row>
  );
}

/** 程序本身：没装的一键安装；装了的看版本（有新版本可以升级）、位置（可以卸载）和配置文件。 */
function ProgramCard({ tool, latest, windows }: { tool: ToolOverview; latest?: ToolLatest; windows: boolean }) {
  const { t } = useTranslation();
  const install = useToolInstall(tool.kind);
  const name = t(`tools.${tool.kind}`);
  const upgrading = install.action === "upgrade";
  const showInstall = !tool.installed || (install.action === "install" && install.outcome?.status === "installed");
  // 卸载前的确认（先看要运行的命令或要删的文件）。工具不见了或者换了位置时关掉。
  const [confirmingUninstall, setConfirmingUninstall] = useState(false);
  useEffect(() => setConfirmingUninstall(false), [tool.installed, tool.binaryPath]);
  const busy = install.running || confirmingUninstall;

  return (
    <SectionCard
      title={t("tools.detail.program")}
      description={tool.installed ? undefined : t("tools.status.notInstalled")}
    >
      {tool.installed && (
        <dl className="divide-y divide-border/70">
          <Row label={t("tools.version")}>
            <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
              <span>{tool.version || "—"}</span>
              {latest?.updateAvailable && latest.latest && !busy && !upgrading ? (
                <>
                  <Badge tone="brand">{t("tools.upgrade.available", { version: latest.latest })}</Badge>
                  <button
                    type="button"
                    onClick={() => {
                      setConfirmingUninstall(false);
                      void install.upgrade(tool.version, latest.latest);
                    }}
                    className="inline-flex items-center gap-1 text-xs font-medium text-brand hover:underline"
                  >
                    <ArrowUpCircle className="h-3.5 w-3.5" aria-hidden />
                    {t("tools.upgrade.button")}
                  </button>
                </>
              ) : (
                latest?.upToDate && <span className="text-xs text-muted-foreground">{t("tools.upgrade.upToDate")}</span>
              )}
            </div>
          </Row>
          <Row label={t("tools.binary")}>
            <span className="break-all font-mono text-xs leading-5">{tool.binaryPath || "—"}</span>
            {!busy && (
              <button type="button" onClick={() => setConfirmingUninstall(true)} className="ml-2 text-xs text-muted-foreground hover:text-danger hover:underline">
                {t("tools.uninstall.button")}
              </button>
            )}
          </Row>
          <Row label={t("tools.config")}>
            <span className="break-all font-mono text-xs leading-5">{tool.configPath}</span>
            {!tool.configExists && <span className="ml-1.5 text-xs text-muted-foreground">{t("tools.configMissing")}</span>}
          </Row>
        </dl>
      )}

      {showInstall && (
        <div className={tool.installed ? "mt-4" : ""}>
          <InstallPanel install={install} command={tool.installCommand} npm={tool.installNpm} windows={windows} />
        </div>
      )}
      {upgrading && (
        <div className="mt-4">
          <UpgradePanel install={install} windows={windows} toolName={name} />
        </div>
      )}
      {confirmingUninstall && tool.installed && !install.running && (
        <div className="mt-4">
          <UninstallConfirm
            tool={tool}
            onCancel={() => setConfirmingUninstall(false)}
            onConfirm={(preview) => {
              setConfirmingUninstall(false);
              void install.uninstall(preview);
            }}
          />
        </div>
      )}
      {install.action === "uninstall" && (
        <div className="mt-4">
          <UninstallPanel install={install} windows={windows} toolName={name} />
        </div>
      )}
    </SectionCard>
  );
}
