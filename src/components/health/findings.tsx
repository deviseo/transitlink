import { useCallback, useState, type ReactNode } from "react";
import { useNavigate } from "react-router";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { CircleAlert, CircleCheck, CircleX, LoaderCircle } from "lucide-react";
import { AddSiteDialog } from "../sites/AddSiteDialog";
import { SetupDialog } from "../tools/SetupDialog";
import { Button } from "../ui/button";
import { switchTools, type SiteView, type ToolKind } from "../../lib/api";
import { useCurrentSite } from "../../lib/currentSite";
import { errorText } from "../../lib/errors";
import { findingText, fixLabel, refreshAlerts, type Finding, type Fix } from "../../lib/health";
import { inDesktop } from "../../lib/tauri";

type FixState = { status: "running" } | { status: "done" } | { status: "failed"; error: unknown };
type Reapply = Extract<Fix, { action: "reapply" }>;

const keyOf = (fix: Fix) => JSON.stringify(fix);

export interface Fixer {
  run: (fix: Fix) => void;
  /** 一个一个重新配置（一键修复）。 */
  runAll: (fixes: Reapply[]) => Promise<void>;
  state: (fix?: Fix) => FixState | undefined;
  /** 重新检查前忘掉修好了的（没修好的留着原因）。 */
  reset: () => void;
  /** 修的时候要打开的窗口（重新登录、换分组），放在页面里。 */
  dialogs: ReactNode;
  /** 有窗口开着（这时外面的窗口不要响应 Esc）。 */
  dialogOpen: boolean;
}

/**
 * 执行修复：重新配置直接做（Key 不能用了会换新的，模型没了会自动换）；重新登录、换分组打开对应的窗口；
 * 去充值打开站点；去安装打开工具页。`onChanged`：重新登录、换完分组以后（例如重新体检）。
 */
export function useFixer(onChanged?: () => void): Fixer {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const { sites } = useCurrentSite();
  const [states, setStates] = useState<Record<string, FixState>>({});
  const [relogin, setRelogin] = useState<SiteView | null>(null);
  const [setup, setSetup] = useState<{ kind: ToolKind; siteId: string } | null>(null);

  const afterChange = useCallback(
    (siteId: string) => {
      void queryClient.invalidateQueries({ queryKey: ["tools"] });
      void queryClient.invalidateQueries({ queryKey: ["site-keys", siteId] });
      void queryClient.invalidateQueries({ queryKey: ["site-account", siteId] });
      void refreshAlerts().then((alerts) => queryClient.setQueryData(["alerts"], alerts));
    },
    [queryClient],
  );

  const reapply = useCallback(
    async (fix: Reapply) => {
      const key = keyOf(fix);
      setStates((current) => ({ ...current, [key]: { status: "running" } }));
      try {
        const report = await switchTools(fix.siteId, [fix.kind]);
        if (report.failures.length > 0) throw report.failures[0].error;
        setStates((current) => ({ ...current, [key]: { status: "done" } }));
      } catch (error) {
        setStates((current) => ({ ...current, [key]: { status: "failed", error } }));
      }
      afterChange(fix.siteId);
    },
    [afterChange],
  );

  const run = (fix: Fix) => {
    const site = "siteId" in fix ? sites.find((item) => item.id === fix.siteId) : undefined;
    switch (fix.action) {
      case "reapply":
        void reapply(fix);
        return;
      case "setup":
        setSetup({ kind: fix.kind, siteId: fix.siteId });
        return;
      case "relogin":
        if (site) setRelogin(site);
        return;
      case "recharge":
        if (site) void (inDesktop ? openUrl(site.baseUrl) : window.open(site.baseUrl, "_blank", "noopener"));
        return;
      case "install":
        void navigate(`/tools/${fix.kind}`);
        return;
    }
  };

  const runAll = async (fixes: Reapply[]) => {
    for (const fix of fixes) await reapply(fix);
  };

  function reset() {
    setStates((current) => Object.fromEntries(Object.entries(current).filter(([, state]) => state.status === "failed")));
  }

  const dialogs = (
    <>
      {relogin && (
        <AddSiteDialog
          open
          initialUrl={relogin.baseUrl}
          initialAccount={relogin.account}
          onClose={() => setRelogin(null)}
          onAdded={() => {
            const id = relogin.id;
            setRelogin(null);
            void queryClient.invalidateQueries({ queryKey: ["sites"] });
            afterChange(id);
            reset();
            onChanged?.();
          }}
        />
      )}
      {setup && (
        <SetupDialog
          open
          only={setup.kind}
          initialSiteId={setup.siteId}
          onClose={() => {
            const id = setup.siteId;
            setSetup(null);
            afterChange(id);
            reset();
            onChanged?.();
          }}
        />
      )}
    </>
  );

  return {
    run,
    runAll,
    state: (fix) => (fix ? states[keyOf(fix)] : undefined),
    reset,
    dialogs,
    dialogOpen: relogin !== null || setup !== null,
  };
}

/** 一个问题：图标、说人话的文案和修复按钮。 */
export function FindingRow({ finding, fixer, prefix }: { finding: Finding; fixer: Fixer; prefix?: string }) {
  const { t, i18n } = useTranslation();
  const state = fixer.state(finding.fix);
  const text = findingText(t, finding, i18n.language);
  return (
    <li className="space-y-1">
      <div className="flex items-start gap-2 text-[13px] leading-5">
        {finding.level === "error" ? (
          <CircleX className="mt-0.5 h-4 w-4 shrink-0 text-danger" aria-label={t("checkup.levelError")} />
        ) : (
          <CircleAlert className="mt-0.5 h-4 w-4 shrink-0 text-warning" aria-label={t("checkup.levelWarn")} />
        )}
        <span className="min-w-0 flex-1">
          {prefix && <span className="font-medium">{prefix}</span>}
          {text}
        </span>
        {finding.fix &&
          (state?.status === "done" ? (
            <span className="flex shrink-0 items-center gap-1 text-xs text-success">
              <CircleCheck className="h-3.5 w-3.5" aria-hidden />
              {t("health.fixed")}
            </span>
          ) : (
            <Button
              size="sm"
              variant={finding.fix.action === "reapply" ? "primary" : "secondary"}
              className="h-7 shrink-0 px-2.5 text-xs"
              disabled={state?.status === "running"}
              onClick={() => finding.fix && fixer.run(finding.fix)}
            >
              {state?.status === "running" && <LoaderCircle className="h-3.5 w-3.5 animate-spin" aria-hidden />}
              {fixLabel(t, finding)}
            </Button>
          ))}
      </div>
      {state?.status === "failed" && <p className="pl-6 text-xs text-danger">{errorText(t, state.error)}</p>}
    </li>
  );
}
