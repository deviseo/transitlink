import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { CircleCheck, CircleX, Copy, LoaderCircle, RefreshCw } from "lucide-react";
import { PageHeader } from "../components/layout/PageHeader";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Card } from "../components/ui/card";
import { InfoRow } from "../components/ui/info-row";
import { detectTools, type ToolKind, type ToolStatus } from "../lib/api";

const installCommands: Record<ToolKind, string> = {
  claudeCode: "npm install -g @anthropic-ai/claude-code",
  codex: "npm install -g @openai/codex",
};

export function ToolsPage() {
  const { t } = useTranslation();
  const { data, isFetching, refetch } = useQuery({ queryKey: ["tools"], queryFn: detectTools });

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
          <ToolCard key={tool.kind} tool={tool} />
        ))}
      </div>
    </div>
  );
}

function ToolCard({ tool }: { tool: ToolStatus }) {
  const { t } = useTranslation();
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

      {!tool.installed && <InstallCommand command={installCommands[tool.kind]} />}

      <div className="mt-auto pt-5">
        <Button className="w-full" disabled>
          {t("tools.configure")}
        </Button>
        <p className="mt-1.5 text-center text-xs text-muted-foreground">{t("tools.configureHint")}</p>
      </div>
    </Card>
  );
}

function InstallCommand({ command }: { command: string }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    await navigator.clipboard.writeText(command);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };
  return (
    <div className="mt-4">
      <p className="text-xs text-muted-foreground">{t("tools.installHint")}</p>
      <div className="mt-1.5 flex items-center gap-2 rounded-lg bg-muted py-1 pl-3 pr-1">
        <code className="min-w-0 flex-1 truncate font-mono text-xs">{command}</code>
        <Button variant="ghost" size="sm" onClick={() => void copy()} aria-label={t("common.copy")}>
          {copied ? t("common.copied") : <Copy className="h-4 w-4" aria-hidden />}
        </Button>
      </div>
    </div>
  );
}
