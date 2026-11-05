import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { installTool, toolVersions, uninstallTool, upgradeTool, type InstallLine, type InstallOutcome, type ToolKind, type ToolStatus, type UninstallPreview } from "./api";
import { inDesktop } from "./tauri";

/** 输出最多留这么多行。 */
const MAX_LINES = 200;

export type InstallAction = "install" | "upgrade" | "uninstall";

/** 一键安装、升级或卸载一个工具：过程中的输出、结果，结束后重新检测工具。 */
export function useToolInstall(kind: ToolKind) {
  const queryClient = useQueryClient();
  const [lines, setLines] = useState<string[]>([]);
  const [outcome, setOutcome] = useState<InstallOutcome | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [running, setRunning] = useState(false);
  const [action, setAction] = useState<InstallAction | null>(null);
  /** 升级前的版本（升级后比较版本变了没有）和要升到的版本。 */
  const [from, setFrom] = useState<string | null>(null);
  const [to, setTo] = useState<string | null>(null);
  /** 用户确认过的卸载预览（重试时再用）。 */
  const [confirmed, setConfirmed] = useState<UninstallPreview | null>(null);

  useEffect(() => {
    if (!inDesktop) return;
    const stop = listen<InstallLine>("tool-install", (event) => {
      if (event.payload.kind === kind) setLines((current) => [...current, event.payload.line].slice(-MAX_LINES));
    });
    return () => void stop.then((unlisten) => unlisten());
  }, [kind]);

  const run = async (next: InstallAction, mirror: boolean, target: string | null, preview: UninstallPreview | null) => {
    setAction(next);
    setRunning(true);
    setLines([]);
    setOutcome(null);
    setError(null);
    try {
      setOutcome(
        await (next === "install"
          ? installTool(kind, mirror)
          : next === "upgrade"
            ? upgradeTool(kind, mirror, target)
            : uninstallTool(kind, preview as UninstallPreview)),
      );
    } catch (caught) {
      setError(caught);
    } finally {
      setRunning(false);
      // 成没成功都重新检测一次（有时命令报错但其实装好了）。
      void queryClient.invalidateQueries({ queryKey: ["tools"] });
    }
  };

  return {
    lines,
    outcome,
    error,
    running,
    action,
    from,
    /** 一键安装。mirror：npm 用国内镜像。 */
    start: (mirror = false) => run("install", mirror, null, null),
    /** 升级。current：现在的版本；target：要升到的版本（查到的最新版）。 */
    upgrade: (current: string | null, target: string | null) => {
      setFrom(current);
      setTo(target);
      return run("upgrade", false, target, null);
    },
    /** 卸载（只卸程序）。preview：用户确认过的预览。 */
    uninstall: (preview: UninstallPreview) => {
      setConfirmed(preview);
      return run("uninstall", false, null, preview);
    },
    /** 再做一次上次的操作。 */
    retry: (mirror = false) => run(action ?? "install", mirror, to, confirmed),
  };
}

/** 已安装工具在 npm 上的最新版本（一小时内不重复查；版本变了重新查）。 */
export function useToolVersions(tools: ToolStatus[] | undefined) {
  const installed = (tools ?? []).flatMap((tool) => (tool.installed && tool.version ? [{ kind: tool.kind, version: tool.version }] : []));
  return useQuery({
    queryKey: ["tool-versions", installed.map((tool) => `${tool.kind}@${tool.version}`)],
    queryFn: () => toolVersions(installed),
    enabled: installed.length > 0,
    staleTime: 60 * 60 * 1000,
  });
}

/** 把目录加到 PATH 的命令（zsh 是 macOS 默认的终端，bash 也给一行）。 */
export function pathCommand(dir: string, shell: "zsh" | "bash"): string {
  const file = shell === "zsh" ? "~/.zshrc" : "~/.bash_profile";
  return `echo 'export PATH="${dir}:$PATH"' >> ${file}`;
}
