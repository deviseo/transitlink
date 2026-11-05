import { useState } from "react";
import { Link } from "react-router";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowRight, CircleCheck, LoaderCircle, Sparkles } from "lucide-react";
import { SetupDialog } from "../tools/SetupDialog";
import { Button, buttonVariants } from "../ui/button";
import { Card } from "../ui/card";
import { detectTools, switchTools, type SiteView, type ToolKind, type ToolOverview } from "../../lib/api";
import { siteName, useCurrentSite } from "../../lib/currentSite";
import { errorText } from "../../lib/errors";

/**
 * 概览里的编程工具：每个工具和当前站点的关系。没配置的可以一键配置；用的是别的站点时，可以改用当前站点
 * （只切这一个工具）。其他情况（没安装、配置被改过）到工具页处理。
 */
export function ToolsSummary({ site }: { site: SiteView }) {
  const { t } = useTranslation();
  const tools = useQuery({ queryKey: ["tools"], queryFn: detectTools });
  const [setup, setSetup] = useState<ToolKind | null>(null);
  return (
    <Card className="space-y-3">
      <div className="flex items-center justify-between gap-3">
        <h2 className="font-medium">{t("overview.tools")}</h2>
        <Link to="/tools" className={buttonVariants({ variant: "ghost", size: "sm" })}>
          {t("overview.viewTools")}
          <ArrowRight className="h-4 w-4" aria-hidden />
        </Link>
      </div>
      {tools.isLoading && <LoaderCircle className="h-4 w-4 animate-spin text-muted-foreground" aria-hidden />}
      <ul className="divide-y divide-border">
        {(tools.data ?? []).map((tool) => (
          <ToolRow key={tool.kind} tool={tool} site={site} onSetup={() => setSetup(tool.kind)} />
        ))}
      </ul>
      <SetupDialog open={setup !== null} only={setup ?? undefined} initialSiteId={site.id} onClose={() => setSetup(null)} />
    </Card>
  );
}

function ToolRow({ tool, site, onSetup }: { tool: ToolOverview; site: SiteView; onSetup: () => void }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { sites } = useCurrentSite();
  const useHere = useMutation({
    mutationFn: async () => {
      const report = await switchTools(site.id, [tool.kind]);
      if (report.failures.length > 0) throw report.failures[0].error;
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: ["tools"] }),
  });
  const applied = tool.applied;
  const elsewhere = applied && applied.siteId !== site.id ? sites.find((item) => item.id === applied.siteId) : undefined;

  let status: string;
  let action = null;
  if (applied && applied.siteId === site.id && tool.state === "configured") {
    status = applied.model
      ? t("overview.hereModel", { group: applied.group ?? t("setup.defaultGroup"), model: applied.model })
      : t("overview.here", { group: applied.group ?? t("setup.defaultGroup") });
  } else if (applied && tool.state === "configured") {
    status = t("overview.elsewhere", { site: elsewhere ? siteName(elsewhere) : applied.siteName });
    action = (
      <Button variant="secondary" size="sm" onClick={() => useHere.mutate()} disabled={useHere.isPending}>
        {useHere.isPending && <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />}
        {t("overview.useHere")}
      </Button>
    );
  } else if (tool.state === "notConfigured") {
    status = tool.installed ? t("overview.notConfigured") : t("overview.notInstalled");
    action = (
      <Button variant={tool.installed ? "primary" : "secondary"} size="sm" onClick={onSetup}>
        <Sparkles className="h-4 w-4" aria-hidden />
        {t("tools.configure")}
      </Button>
    );
  } else {
    // 配置被改过、用着别的服务、文件格式有误：到工具页看详情。
    status = t(`tools.state.${tool.state}`, { site: applied?.siteName ?? "", target: tool.currentBaseUrl ?? tool.currentProvider ?? "" });
  }

  const ok = applied?.siteId === site.id && tool.state === "configured";
  return (
    <li className="flex flex-wrap items-center gap-x-3 gap-y-2 py-2.5 first:pt-0 last:pb-0">
      <div className="min-w-0 flex-1">
        <p className="flex items-center gap-1.5 text-sm font-medium">
          {t(`tools.${tool.kind}`)}
          {ok && <CircleCheck className="h-4 w-4 text-success" aria-hidden />}
        </p>
        <p className="truncate text-xs text-muted-foreground">{status}</p>
        {useHere.isError && <p className="text-xs text-danger">{errorText(t, useHere.error)}</p>}
      </div>
      {action}
    </li>
  );
}
