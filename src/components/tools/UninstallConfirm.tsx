import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { CircleAlert, LoaderCircle, Trash2 } from "lucide-react";
import { Button } from "../ui/button";
import { uninstallPlan, type ToolOverview, type UninstallPreview } from "../../lib/api";
import { errorText } from "../../lib/errors";
import { CopyCommand } from "./InstallPanel";

/**
 * 卸载前的确认：先把要运行的命令或要删的文件给用户看。认不出是怎么装的（或者是 pnpm 等装的、随桌面版
 * 装的）就不提供卸载按钮，只说明怎么自己卸。
 */
export function UninstallConfirm({
  tool,
  onCancel,
  onConfirm,
}: {
  tool: ToolOverview;
  onCancel: () => void;
  onConfirm: (preview: UninstallPreview) => void;
}) {
  const { t } = useTranslation();
  const name = t(`tools.${tool.kind}`);
  // 每次打开都重新检测：安装情况可能刚变过。
  const preview = useQuery({ queryKey: ["uninstall-plan", tool.kind], queryFn: () => uninstallPlan(tool.kind), staleTime: 0, gcTime: 0 });
  const data = preview.data;
  const runnable = data != null && data.plan.kind !== "manual";
  const configured = tool.state === "configured" || tool.state === "changed";

  return (
    <div className="space-y-3 rounded-lg border border-border p-3">
      <p className="text-sm font-medium">{t("tools.uninstall.title", { tool: name })}</p>
      {preview.isPending ? (
        <p className="flex items-center gap-1.5 text-sm text-muted-foreground">
          <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />
          {t("tools.uninstall.loading")}
        </p>
      ) : preview.isError ? (
        <p className="text-sm text-danger">{errorText(t, preview.error)}</p>
      ) : data == null ? (
        <p className="text-sm text-muted-foreground">{t("tools.uninstall.notInstalled", { tool: name })}</p>
      ) : (
        <PlanDetails preview={data} toolName={name} />
      )}
      {runnable && (
        <p className="text-xs text-muted-foreground">
          {t("tools.uninstall.keeps")}
          {configured && ` ${t("tools.uninstall.keepsConfigured")}`}
        </p>
      )}
      <div className="flex justify-end gap-2">
        <Button variant="ghost" size="sm" onClick={onCancel}>
          {runnable ? t("common.cancel") : t("common.close")}
        </Button>
        {runnable && (
          <Button size="sm" className="bg-danger" onClick={() => onConfirm(data)}>
            <Trash2 className="h-4 w-4" aria-hidden />
            {t("tools.uninstall.confirm")}
          </Button>
        )}
      </div>
    </div>
  );
}

function PlanDetails({ preview, toolName }: { preview: UninstallPreview; toolName: string }) {
  const { t } = useTranslation();
  const { plan } = preview;
  switch (plan.kind) {
    case "command":
      return (
        <div className="space-y-1.5">
          <p className="text-xs text-muted-foreground">{t("tools.uninstall.runs")}</p>
          <code className="block break-all rounded-lg bg-muted px-3 py-2 font-mono text-xs">{plan.display}</code>
        </div>
      );
    case "files":
      return (
        <div className="space-y-1.5">
          <p className="text-xs text-muted-foreground">{t("tools.uninstall.removes")}</p>
          <ul className="space-y-0.5 rounded-lg bg-muted px-3 py-2 font-mono text-xs">
            {plan.files.map((file) => (
              <li key={file} className="break-all">
                {file}
              </li>
            ))}
          </ul>
        </div>
      );
    case "manual":
      if (plan.command) {
        return (
          <div className="space-y-1.5">
            <p className="text-sm text-muted-foreground">{t("tools.uninstall.manualCommand", { tool: toolName, manager: plan.command.split(" ")[0] })}</p>
            <CopyCommand command={plan.command} />
          </div>
        );
      }
      return (
        <p className="flex items-start gap-1.5 break-all text-sm text-muted-foreground">
          <CircleAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
          {plan.desktop ? t("tools.uninstall.desktop", { tool: toolName }) : t("tools.uninstall.unknown", { tool: toolName, path: preview.binary })}
        </p>
      );
  }
}
