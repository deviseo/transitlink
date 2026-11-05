import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowRightLeft, CircleAlert, CircleCheck, CircleX, LoaderCircle, RefreshCw, RotateCcw, Sparkles, Zap } from "lucide-react";
import { PageHeader } from "../components/layout/PageHeader";
import { InstallPanel } from "../components/tools/InstallPanel";
import { SetupDialog } from "../components/tools/SetupDialog";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Card } from "../components/ui/card";
import { InfoRow } from "../components/ui/info-row";
import { detectTools, getAppInfo, listSites, restoreTool, setToolModel, switchTools, testTool, toolModels, type ToolKind, type ToolOverview } from "../lib/api";
import { siteName, useCurrentSite } from "../lib/currentSite";
import { errorText } from "../lib/errors";
import { useToolInstall } from "../lib/install";
import { testText } from "../lib/tools";

export function ToolsPage() {
  const { t } = useTranslation();
  const { data, isFetching, refetch } = useQuery({ queryKey: ["tools"], queryFn: detectTools });
  const sites = useQuery({ queryKey: ["sites"], queryFn: listSites });
  const app = useQuery({ queryKey: ["app-info"], queryFn: getAppInfo });
  const [setup, setSetup] = useState<ToolKind | null>(null);
  const hasSites = (sites.data ?? []).length > 0;
  const { site: current } = useCurrentSite();

  return (
    <div className="space-y-6">
      <PageHeader
        title={t("tools.title")}
        subtitle={t("tools.subtitle")}
        action={
          <Button variant="secondary" size="sm" onClick={() => void refetch()} disabled={isFetching}>
            {isFetching ? <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden /> : <RefreshCw className="h-4 w-4" aria-hidden />}
            {t("common.refresh")}
          </Button>
        }
      />
      <div className="grid gap-4 md:grid-cols-2">
        {(data ?? []).map((tool) => (
          <ToolCard key={tool.kind} tool={tool} platform={app.data?.platform} hasSites={hasSites} onSetup={() => setSetup(tool.kind)} />
        ))}
      </div>
      <SetupDialog open={setup !== null} only={setup ?? undefined} initialSiteId={current?.id} onClose={() => setSetup(null)} />
    </div>
  );
}

function ToolCard({ tool, platform, hasSites, onSetup }: { tool: ToolOverview; platform?: string; hasSites: boolean; onSetup: () => void }) {
  const { t } = useTranslation();
  // 一键安装：装好后结果（和终端 PATH 的提示）留在卡片上，直到离开这个页面。
  const install = useToolInstall(tool.kind);
  const showInstall = !tool.installed || install.outcome?.status === "installed";
  return (
    <Card className="flex flex-col">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="font-medium">{t(`tools.${tool.kind}`)}</h2>
          <p className="mt-0.5 text-xs text-muted-foreground">{t(`tools.${tool.kind}Desc`)}</p>
        </div>
        {tool.installed ? (
          <Badge tone="success">
            <CircleCheck className="h-3.5 w-3.5" aria-hidden />
            {t("tools.installed")}
          </Badge>
        ) : (
          <Badge tone="danger">
            <CircleX className="h-3.5 w-3.5" aria-hidden />
            {t("tools.notInstalled")}
          </Badge>
        )}
      </div>

      <dl className="mt-4 space-y-2 text-sm">
        {tool.installed && <InfoRow label={t("tools.version")} value={tool.version} />}
        {tool.installed && <InfoRow label={t("tools.binary")} value={tool.binaryPath} mono />}
        <InfoRow label={t("tools.config")} value={tool.configExists ? tool.configPath : `${tool.configPath} ${t("tools.configMissing")}`} mono />
      </dl>

      {showInstall && (
        <div className="mt-4">
          <InstallPanel install={install} command={tool.installCommand} npm={tool.installNpm} windows={platform === "windows"} />
        </div>
      )}

      <div className="mt-auto pt-5">
        <ConfigSection tool={tool} hasSites={hasSites} onSetup={onSetup} />
      </div>
    </Card>
  );
}

/** 配置状态和操作：一键配置、测试连接、换模型、还原。 */
function ConfigSection({ tool, hasSites, onSetup }: { tool: ToolOverview; hasSites: boolean; onSetup: () => void }) {
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
  // 用的不是左上角的当前站点：可以只把这个工具改用当前站点。
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

  if (tool.state === "invalid") {
    return (
      <p className="flex items-start gap-1.5 rounded-lg bg-danger/10 p-3 text-sm text-danger">
        <CircleX className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
        {t("tools.state.invalid")}
      </p>
    );
  }

  return (
    <div className="space-y-3">
      {tool.state === "configured" && tool.applied && (
        <div className="rounded-lg bg-muted/60 p-3 text-sm">
          <p className="flex items-center gap-1.5 font-medium text-success">
            <CircleCheck className="h-4 w-4 shrink-0" aria-hidden />
            {t("tools.state.configured", { site: appliedSite ? siteName(appliedSite) : tool.applied.siteName })}
          </p>
          <dl className="mt-2 space-y-1.5">
            <InfoRow label={t("tools.key")} value={tool.applied.keyName} />
            <InfoRow label={t("tools.group")} value={tool.applied.group} />
            <ModelRow tool={tool} onChanged={refresh} />
          </dl>
          {tool.kind === "geminiCli" && <p className="mt-2 text-xs text-muted-foreground">{t("tools.geminiTrust")}</p>}
          {elsewhere && current && (
            <div className="mt-3 flex flex-wrap items-center justify-between gap-2 border-t border-border pt-3">
              <p className="text-xs text-muted-foreground">{t("tools.notCurrent", { site: siteName(current) })}</p>
              <Button variant="secondary" size="sm" onClick={() => useCurrent.mutate()} disabled={useCurrent.isPending}>
                {useCurrent.isPending ? <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden /> : <ArrowRightLeft className="h-4 w-4" aria-hidden />}
                {t("tools.useCurrent", { site: siteName(current) })}
              </Button>
              {useCurrent.isError && <p className="w-full text-xs text-danger">{errorText(t, useCurrent.error)}</p>}
            </div>
          )}
        </div>
      )}
      {tool.state === "changed" && (
        <p className="flex items-start gap-1.5 text-sm text-warning">
          <CircleAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
          {t("tools.state.changed")}
        </p>
      )}
      {tool.state === "other" && (
        <p className="break-all text-sm text-muted-foreground">{t("tools.state.other", { target: tool.currentBaseUrl ?? tool.currentProvider ?? "" })}</p>
      )}

      {test.data && <p className={`text-sm ${test.data.status === "ok" ? "text-success" : "text-danger"}`}>{testText(t, test.data)}</p>}
      {test.isError && <p className="text-sm text-danger">{errorText(t, test.error)}</p>}
      {restore.isError && <p className="text-sm text-danger">{errorText(t, restore.error)}</p>}

      {confirming ? (
        <div className="space-y-2 rounded-lg border border-border p-3">
          <p className="text-sm font-medium">{t("tools.restoreConfirm")}</p>
          <p className="text-xs text-muted-foreground">{t("tools.restoreHint")}</p>
          <div className="flex justify-end gap-2">
            <Button variant="ghost" size="sm" onClick={() => setConfirming(false)}>
              {t("common.cancel")}
            </Button>
            <Button size="sm" className="bg-danger" onClick={() => restore.mutate()} disabled={restore.isPending}>
              {restore.isPending && <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />}
              {t("tools.restore")}
            </Button>
          </div>
        </div>
      ) : (
        <div className="flex flex-wrap items-center gap-2">
          {/* 还没装的工具，先一键安装（上面），配置按钮放次要位置。 */}
          <Button size="sm" variant={tool.installed ? "primary" : "secondary"} onClick={onSetup} disabled={!hasSites}>
            <Sparkles className="h-4 w-4" aria-hidden />
            {ours ? t("tools.reconfigure") : t("tools.configure")}
          </Button>
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
          {!hasSites && <span className="text-xs text-muted-foreground">{t("tools.needSite")}</span>}
        </div>
      )}
    </div>
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
    <div className="flex items-center gap-3">
      <dt className="w-20 shrink-0 text-muted-foreground">{t("tools.model")}</dt>
      <dd className="flex min-w-0 flex-1 items-center gap-2">
        {!editing ? (
          <>
            <span className="min-w-0 truncate">{current ?? t("tools.defaultModel")}</span>
            <button type="button" onClick={() => setEditing(true)} className="shrink-0 text-xs text-brand hover:underline">
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
            className="h-8 min-w-0 flex-1 rounded-lg border border-border bg-card px-2 text-sm outline-none focus:border-brand focus:ring-1 focus:ring-brand"
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
      </dd>
    </div>
  );
}
