import { useState } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { CircleAlert, CircleCheck, Copy, Download, LoaderCircle, RefreshCw } from "lucide-react";
import { Button } from "../ui/button";
import { isCommandError, type InstallOutcome } from "../../lib/api";
import { errorText } from "../../lib/errors";
import { pathCommand, type InstallAction, type useToolInstall } from "../../lib/install";

const NODE_DOWNLOAD = "https://nodejs.org/zh-cn/download";

type Install = ReturnType<typeof useToolInstall>;

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

/** 安装（升级）输出的最后几行。 */
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

/** 正在运行：转圈和最后几行输出。 */
function Running({ install, mode }: { install: Install; mode: InstallAction }) {
  const { t } = useTranslation();
  return (
    <div className="space-y-2" role="status">
      <p className="flex items-center gap-2 text-sm">
        <LoaderCircle className="h-4 w-4 animate-spin text-brand" aria-hidden />
        {t(mode === "install" ? "tools.install.running" : "tools.upgrade.running")}
      </p>
      {install.lines.length > 0 && <InstallLog lines={install.lines} />}
    </div>
  );
}

/** 命令没跑起来（例如正在装别的工具）时的说明。 */
function StartError({ install }: { install: Install }) {
  const { t } = useTranslation();
  if (install.error == null) return null;
  const busy = isCommandError(install.error) && install.error.code === "installBusy";
  return (
    <p className="text-sm text-danger" role="alert">
      {busy ? t("errors.installBusy") : errorText(t, install.error)}
    </p>
  );
}

/** 让用户自己在终端里运行的命令。 */
function ManualCommand({ mode, command, windows }: { mode: InstallAction; command: string; windows: boolean }) {
  const { t } = useTranslation();
  if (!command) return null;
  const hint =
    mode === "install"
      ? t(windows ? "tools.installHintWindows" : "tools.installHint")
      : t(windows ? "tools.upgrade.hintWindows" : "tools.upgrade.hint");
  return (
    <div className="space-y-1.5">
      <p className="text-xs text-muted-foreground">{hint}</p>
      <CopyCommand command={command} />
    </div>
  );
}

/** 重试；npm 命令还可以换国内镜像重试。 */
function RetryButtons({ install, npm }: { install: Install; npm: boolean }) {
  const { t } = useTranslation();
  return (
    <div className="flex flex-wrap gap-2">
      <Button variant="secondary" size="sm" onClick={() => void install.retry(false)}>
        <RefreshCw className="h-4 w-4" aria-hidden />
        {t("tools.install.retry")}
      </Button>
      {npm && (
        <Button variant="secondary" size="sm" onClick={() => void install.retry(true)}>
          {t("tools.install.retryMirror")}
        </Button>
      )}
    </div>
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
  install: Install;
  /** 手动安装的命令。 */
  command: string;
  /** 用 npm 安装（可以换国内镜像）。 */
  npm: boolean;
  windows: boolean;
}) {
  const { t } = useTranslation();
  // 只显示安装的过程和结果；上次做的是升级（升级失败后检测不到了）时从头显示安装按钮。
  const mine = install.action === "install";
  if (mine && install.running) return <Running install={install} mode="install" />;
  if (!mine || !install.outcome) {
    return (
      <div className="space-y-3">
        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" onClick={() => void install.start(false)} disabled={install.running}>
            <Download className="h-4 w-4" aria-hidden />
            {t("tools.install.button")}
          </Button>
          <span className="text-xs text-muted-foreground">{t("tools.install.hint")}</span>
        </div>
        {mine && <StartError install={install} />}
        <ManualCommand mode="install" command={command} windows={windows} />
      </div>
    );
  }
  return <Outcome mode="install" install={install} outcome={install.outcome} command={command} npm={npm} windows={windows} />;
}

/** 升级：显示输出；完成后说明升到了哪个版本；失败时说明原因，给出重试和可以复制到终端的升级命令。 */
export function UpgradePanel({ install, windows, toolName }: { install: Install; windows: boolean; toolName: string }) {
  if (install.running) return <Running install={install} mode="upgrade" />;
  if (!install.outcome) {
    return (
      <div className="space-y-2">
        <StartError install={install} />
        <RetryButtons install={install} npm={false} />
      </div>
    );
  }
  // 升级命令事先不知道（按工具是怎么装的选），结果里带着。
  const command = ("command" in install.outcome ? install.outcome.command : null) ?? "";
  return (
    <Outcome
      mode="upgrade"
      install={install}
      outcome={install.outcome}
      command={command}
      npm={/(^|\s)npm\s/.test(command)}
      windows={windows}
      toolName={toolName}
    />
  );
}

function Outcome({
  mode,
  install,
  outcome,
  command,
  npm,
  windows,
  toolName = "",
}: {
  mode: InstallAction;
  install: Install;
  outcome: InstallOutcome;
  command: string;
  npm: boolean;
  windows: boolean;
  toolName?: string;
}) {
  const { t } = useTranslation();
  const problem = (text: string) => (
    <p className="flex items-start gap-1.5 text-sm text-danger" role="alert">
      <CircleAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
      {text}
    </p>
  );
  const manual = <ManualCommand mode={mode} command={("command" in outcome ? outcome.command : null) || command} windows={windows} />;
  const retry = <RetryButtons install={install} npm={npm} />;
  switch (outcome.status) {
    case "installed": {
      const { version } = outcome;
      const text =
        mode === "install"
          ? version
            ? t("tools.install.installed", { version })
            : t("tools.install.installedNoVersion")
          : !version
            ? t("tools.upgrade.doneNoVersion")
            : version === install.from
              ? t("tools.upgrade.unchanged", { version })
              : t("tools.upgrade.done", { version });
      return (
        <div className="space-y-2">
          <p className="flex items-center gap-1.5 text-sm font-medium text-success" role="status">
            <CircleCheck className="h-4 w-4 shrink-0" aria-hidden />
            {text}
          </p>
          {/* 升级前就能用，不再提示 PATH。 */}
          {mode === "install" &&
            outcome.pathHint &&
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
    }
    case "manual":
      return outcome.command ? (
        <div className="space-y-1.5" role="status">
          <p className="text-sm text-muted-foreground">
            {t("tools.upgrade.manualCommand", { tool: toolName, manager: outcome.command.split(" ")[0] })}
          </p>
          <CopyCommand command={outcome.command} />
        </div>
      ) : (
        <p className="flex items-start gap-1.5 text-sm text-muted-foreground" role="status">
          <CircleAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
          {t("tools.upgrade.manual", { tool: toolName })}
        </p>
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
    case "failed": {
      const keys =
        mode === "install"
          ? { network: "tools.install.network", timedOut: "tools.install.timedOut", failed: "tools.install.failed" }
          : { network: "tools.upgrade.network", timedOut: "tools.upgrade.timedOut", failed: "tools.upgrade.failed" };
      return (
        <div className="space-y-3">
          {problem(outcome.status === "failed" ? t(keys.failed, { code: outcome.code ?? "?" }) : t(keys[outcome.status]))}
          {install.lines.length > 0 && <InstallLog lines={install.lines} />}
          {retry}
          {manual}
        </div>
      );
    }
  }
}
