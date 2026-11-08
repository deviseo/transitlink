import { useState } from "react";
import { Link } from "react-router";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { ChevronRight, LoaderCircle, RefreshCw, Sparkles, Stethoscope } from "lucide-react";
import { CheckupDialog } from "../components/health/CheckupDialog";
import { PageHeader } from "../components/layout/PageHeader";
import { SetupDialog } from "../components/tools/SetupDialog";
import { ToolIcon } from "../components/tools/ToolIcon";
import { Badge, Dot } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Segmented } from "../components/ui/segmented";
import { detectTools, type ToolLatest, type ToolOverview } from "../lib/api";
import { useCurrentSite } from "../lib/currentSite";
import { useToolVersions } from "../lib/install";
import { toolStatus } from "../lib/toolStatus";

type Filter = "all" | "installed" | "missing";

/** 工具列表：每个工具一个小卡片（名字、和当前站点的关系、有没有新版本），点进去看详情和操作。 */
export function ToolsPage() {
  const { t } = useTranslation();
  const { data, isFetching, refetch } = useQuery({ queryKey: ["tools"], queryFn: detectTools });
  const versions = useToolVersions(data);
  const { site: current, sites } = useCurrentSite();
  const [filter, setFilter] = useState<Filter>("all");
  const [setupAll, setSetupAll] = useState(false);
  const [checking, setChecking] = useState(false);
  const tools = data ?? [];
  const installed = tools.filter((tool) => tool.installed).length;
  const shown = tools.filter((tool) => filter === "all" || (filter === "installed") === tool.installed);

  return (
    <div className="space-y-5">
      <PageHeader
        title={t("tools.title")}
        subtitle={t("tools.subtitle")}
        action={
          <>
            <Button variant="secondary" size="sm" onClick={() => void refetch()} disabled={isFetching}>
              {isFetching ? <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden /> : <RefreshCw className="h-4 w-4" aria-hidden />}
              {t("common.refresh")}
            </Button>
            <Button variant="secondary" size="sm" onClick={() => setChecking(true)}>
              <Stethoscope className="h-4 w-4" aria-hidden />
              {t("checkup.button")}
            </Button>
            <Button size="sm" onClick={() => setSetupAll(true)} disabled={sites.length === 0} title={sites.length === 0 ? t("tools.needSite") : undefined}>
              <Sparkles className="h-4 w-4" aria-hidden />
              {t("tools.configure")}
            </Button>
          </>
        }
      />
      <Segmented
        label={t("tools.filter.label")}
        value={filter}
        onChange={setFilter}
        options={[
          { value: "all", label: t("tools.filter.all"), count: tools.length },
          { value: "installed", label: t("tools.filter.installed"), count: installed },
          { value: "missing", label: t("tools.filter.missing"), count: tools.length - installed },
        ]}
      />
      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
        {shown.map((tool) => (
          <ToolTile
            key={tool.kind}
            tool={tool}
            latest={versions.data?.find((item) => item.kind === tool.kind)}
            status={toolStatus(t, tool, current, sites)}
          />
        ))}
      </div>
      {data && shown.length === 0 && <p className="py-10 text-center text-sm text-muted-foreground">{t("tools.filter.empty")}</p>}
      <SetupDialog open={setupAll} initialSiteId={current?.id} onClose={() => setSetupAll(false)} />
      <CheckupDialog open={checking} onClose={() => setChecking(false)} />
    </div>
  );
}

function ToolTile({ tool, latest, status }: { tool: ToolOverview; latest?: ToolLatest; status: ReturnType<typeof toolStatus> }) {
  const { t } = useTranslation();
  return (
    <Link
      to={`/tools/${tool.kind}`}
      className="group flex flex-col gap-3.5 rounded-2xl border border-border/80 bg-card p-4 transition hover:border-brand/30 hover:shadow-[0_8px_24px_-12px_rgba(16,24,40,0.28)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand/60"
    >
      <div className="flex items-start gap-3">
        <ToolIcon kind={tool.kind} />
        <div className="min-w-0 flex-1">
          <p className="truncate text-[14px] font-semibold">{t(`tools.${tool.kind}`)}</p>
          <p className="truncate text-xs text-muted-foreground">{t(`tools.vendor.${tool.kind}`)}</p>
        </div>
        <ChevronRight className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground/50 transition group-hover:translate-x-0.5 group-hover:text-foreground" aria-hidden />
      </div>
      <div className="flex min-h-5 items-center justify-between gap-2 text-xs">
        <span className="flex min-w-0 items-center gap-1.5 text-muted-foreground">
          <Dot tone={status.tone} />
          <span className="truncate">{status.text}</span>
        </span>
        {latest?.updateAvailable && <Badge tone="brand">{t("tools.tile.update")}</Badge>}
      </div>
    </Link>
  );
}
