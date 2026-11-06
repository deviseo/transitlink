import { useState } from "react";
import { Link } from "react-router";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowRight, LoaderCircle, Sparkles } from "lucide-react";
import { SetupDialog } from "../tools/SetupDialog";
import { ToolIcon } from "../tools/ToolIcon";
import { Dot } from "../ui/badge";
import { Button, buttonVariants } from "../ui/button";
import { SectionCard } from "../ui/card";
import { detectTools, switchTools, type SiteView, type ToolOverview } from "../../lib/api";
import { useCurrentSite } from "../../lib/currentSite";
import { errorText } from "../../lib/errors";
import { toolStatus } from "../../lib/toolStatus";

/** 概览最多列这么多个工具，其余的在工具页。 */
const MAX_SHOWN = 5;

/** 概览列连接过的和装了的工具（连接过的在前）；一个都没有时列 Claude Code 和 Codex。 */
function shown(tools: ToolOverview[]): ToolOverview[] {
  const connected = tools.filter((tool) => tool.applied);
  const installed = tools.filter((tool) => tool.installed && !tool.applied);
  const used = [...connected, ...installed];
  return used.length > 0 ? used : tools.filter((tool) => tool.kind === "claudeCode" || tool.kind === "codex");
}

/**
 * 概览里的编程工具：每个工具和当前站点的关系。没连接的可以一键配置；用的是别的站点时，可以改用当前站点
 * （只切这一个工具）。其他情况到工具详情里处理。
 */
export function ToolsSummary({ site }: { site: SiteView }) {
  const { t } = useTranslation();
  const tools = useQuery({ queryKey: ["tools"], queryFn: detectTools });
  const [setup, setSetup] = useState<ToolOverview["kind"] | null>(null);
  const list = shown(tools.data ?? []);
  return (
    <SectionCard
      title={t("overview.tools")}
      action={
        <Link to="/tools" className={buttonVariants({ variant: "ghost", size: "sm" })}>
          {t("overview.viewTools")}
          <ArrowRight className="h-4 w-4" aria-hidden />
        </Link>
      }
    >
      {tools.isLoading && <LoaderCircle className="h-4 w-4 animate-spin text-muted-foreground" aria-hidden />}
      <ul className="-mx-2">
        {list.slice(0, MAX_SHOWN).map((tool) => (
          <ToolRow key={tool.kind} tool={tool} site={site} onSetup={() => setSetup(tool.kind)} />
        ))}
      </ul>
      <SetupDialog open={setup !== null} only={setup ?? undefined} initialSiteId={site.id} onClose={() => setSetup(null)} />
    </SectionCard>
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
  const status = toolStatus(t, tool, site, sites);
  const applied = tool.applied;

  let action = null;
  if (applied && tool.state === "configured" && tool.installed && applied.siteId !== site.id) {
    action = (
      <Button variant="secondary" size="sm" onClick={() => useHere.mutate()} disabled={useHere.isPending}>
        {useHere.isPending && <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />}
        {t("overview.useHere")}
      </Button>
    );
  } else if (tool.installed && tool.configurable && tool.state === "notConfigured") {
    action = (
      <Button size="sm" onClick={onSetup}>
        <Sparkles className="h-4 w-4" aria-hidden />
        {t("tools.configure")}
      </Button>
    );
  }

  return (
    <li className="flex items-center gap-3 rounded-xl px-2 py-2 transition hover:bg-muted/60">
      <Link to={`/tools/${tool.kind}`} className="flex min-w-0 flex-1 items-center gap-3 focus-visible:outline-none">
        <ToolIcon kind={tool.kind} size="sm" />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[13px] font-medium">{t(`tools.${tool.kind}`)}</span>
          <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
            <Dot tone={status.tone} />
            <span className="truncate">{status.text}</span>
          </span>
        </span>
      </Link>
      {action}
      {useHere.isError && <p className="text-xs text-danger">{errorText(t, useHere.error)}</p>}
    </li>
  );
}
