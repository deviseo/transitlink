import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { CircleAlert, CircleCheck, Copy, Download, LoaderCircle, RefreshCw } from "lucide-react";
import { Button } from "../ui/button";
import { isCommandError, type InstallOutcome } from "../../lib/api";
import { errorText } from "../../lib/errors";
import { pathCommand, type useToolInstall } from "../../lib/install";

const NODE_DOWNLOAD = "https://nodejs.org/zh-cn/download";

/** 一行可以复制的命令。 */
export function CopyCommand({ command }: { command: string }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    await navigator.clipboard.writeText(command);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };
  return (
    <div className="flex items-center gap-2 rounded-lg bg-muted py-1 pl-3 pr-1">
      <code className="min-w-0 flex-1 truncate font-mono text-xs" title={command}>
        {command}
      </code>
      <Button variant="ghost" size="sm" onClick={() => void copy()} aria-label={t("common.copy")}>
        {copied ? t("common.copied") : <Copy className="h-4 w-4" aria-hidden />}
      </Button>
    </div>
  );
}

/** 安装输出的最后几行。 */
function InstallLog({ lines }: { lines: string[] }) {
  const { t } = useTranslation();
  return (
    <pre
      aria-label={t("tools.install.log")}
      className="max-h-32 overflow-y-auto whitespace-pre-wrap break-all rounded-lg bg-muted p-2 font-mono text-[11px] leading-4 text-muted-foreground"
    >
      {lines.slice(-12).join("\n")}
    </pre>
  );
}

/**
 * 一键安装：运行官方安装命令（或 npm），显示安装输出；装不上时说明原因，给出重试和可以复制到终端的命令。
 * 装好后如果终端里还找不到命令，提示把目录加到 PATH。
 */
export function InstallPanel({
  install,
  command,
  npm,
  windows,
}: {
  install: ReturnType<typeof useToolInstall>;
  /** 手动安装的命令。 */
  command: string;
  /** 用 npm 安装（可以换国内镜像）。 */
  npm: boolean;
  windows: boolean;
}) {
  const { t } = useTranslation();
  const { outcome, running } = install;
  const busy = isCommandError(install.error) && install.error.code === "installBusy";
  const manual = (
    <div className="space-y-1.5">
      <p className="text-xs text-muted-foreground">{windows ? t("tools.installHintWindows") : t("tools.installHint")}</p>
      <CopyCommand command={outcome && "command" in outcome ? outcome.command : command} />
    </div>
  );
  const retry = (
    <div className="flex flex-wrap gap-2">
      <Button variant="secondary" size="sm" onClick={() => void install.start(false)}>
        <RefreshCw className="h-4 w-4" aria-hidden />
        {t("tools.install.retry")}
      </Button>
      {npm && (
        <Button variant="secondary" size="sm" onClick={() => void install.start(true)}>
          {t("tools.install.retryMirror")}
        </Button>
      )}
    </div>
  );

  if (running) {
    return (
      <div className="space-y-2" role="status">
        <p className="flex items-center gap-2 text-sm">
          <LoaderCircle className="h-4 w-4 animate-spin text-brand" aria-hidden />
          {t("tools.install.running")}
        </p>
        {install.lines.length > 0 && <InstallLog lines={install.lines} />}
      </div>
    );
  }

  if (!outcome) {
    return (
      <div className="space-y-3">
        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" onClick={() => void install.start(false)}>
            <Download className="h-4 w-4" aria-hidden />
            {t("tools.install.button")}
          </Button>
          <span className="text-xs text-muted-foreground">{t("tools.install.hint")}</span>
        </div>
        {(busy || install.error != null) && (
          <p className="text-sm text-danger" role="alert">
            {busy ? t("errors.installBusy") : errorText(t, install.error)}
          </p>
        )}
        {manual}
      </div>
    );
  }

  return <Outcome outcome={outcome} lines={install.lines} windows={windows} manual={manual} retry={retry} />;
}

function Outcome({
  outcome,
  lines,
  windows,
  manual,
  retry,
}: {
  outcome: InstallOutcome;
  lines: string[];
  windows: boolean;
  manual: ReactNode;
  retry: ReactNode;
}) {
  const { t } = useTranslation();
  const problem = (text: string) => (
    <p className="flex items-start gap-1.5 text-sm text-danger" role="alert">
      <CircleAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
      {text}
    </p>
  );
  switch (outcome.status) {
    case "installed":
      return (
        <div className="space-y-2">
          <p className="flex items-center gap-1.5 text-sm font-medium text-success" role="status">
            <CircleCheck className="h-4 w-4 shrink-0" aria-hidden />
            {outcome.version ? t("tools.install.installed", { version: outcome.version }) : t("tools.install.installedNoVersion")}
          </p>
          {outcome.pathHint &&
            (windows ? (
              <p className="text-xs text-muted-foreground">{t("tools.install.pathHintWindows")}</p>
            ) : (
              <div className="space-y-1.5 rounded-lg border border-warning/30 bg-warning/10 p-3">
                <p className="text-xs text-warning">{t("tools.install.pathHint", { dir: outcome.pathHint })}</p>
                <CopyCommand command={pathCommand(outcome.pathHint, "zsh")} />
              </div>
            ))}
        </div>
      );
    case "needsNode":
      return (
        <div className="space-y-2">
          {problem(
            t("tools.install.needsNode", {
              required: outcome.required,
              found: outcome.found ? t("tools.install.nodeFound", { version: outcome.found }) : t("tools.install.nodeMissing"),
            }),
          )}
          <div className="flex flex-wrap gap-2">
            <Button size="sm" onClick={() => void openUrl(NODE_DOWNLOAD)}>
              {t("tools.install.downloadNode")}
            </Button>
            {retry}
          </div>
        </div>
      );
    case "npmPermission":
      return (
        <div className="space-y-2">
          {problem(t("tools.install.npmPermission"))}
          <CopyCommand command={outcome.command} />
        </div>
      );
    case "network":
    case "timedOut":
    case "failed":
      return (
        <div className="space-y-3">
          {problem(
            outcome.status === "network"
              ? t("tools.install.network")
              : outcome.status === "timedOut"
                ? t("tools.install.timedOut")
                : t("tools.install.failed", { code: outcome.code ?? "?" }),
          )}
          {lines.length > 0 && <InstallLog lines={lines} />}
          {retry}
          {manual}
        </div>
      );
  }
}
