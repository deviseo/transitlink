import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { installTool, type InstallLine, type InstallOutcome, type ToolKind } from "./api";
import { inDesktop } from "./tauri";

/** 安装输出最多留这么多行。 */
const MAX_LINES = 200;

/** 一键安装一个工具：安装过程的输出、结果，装好后重新检测工具。 */
export function useToolInstall(kind: ToolKind) {
  const queryClient = useQueryClient();
  const [lines, setLines] = useState<string[]>([]);
  const [outcome, setOutcome] = useState<InstallOutcome | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [running, setRunning] = useState(false);

  useEffect(() => {
    if (!inDesktop) return;
    const stop = listen<InstallLine>("tool-install", (event) => {
      if (event.payload.kind === kind) setLines((current) => [...current, event.payload.line].slice(-MAX_LINES));
    });
    return () => void stop.then((unlisten) => unlisten());
  }, [kind]);

  const start = async (mirror = false) => {
    setRunning(true);
    setLines([]);
    setOutcome(null);
    setError(null);
    try {
      const result = await installTool(kind, mirror);
      setOutcome(result);
    } catch (caught) {
      setError(caught);
    } finally {
      setRunning(false);
      // 装没装上都重新检测一次（有时命令报错但其实装好了）。
      void queryClient.invalidateQueries({ queryKey: ["tools"] });
    }
  };

  return { lines, outcome, error, running, start };
}

/** 把目录加到 PATH 的命令（zsh 是 macOS 默认的终端，bash 也给一行）。 */
export function pathCommand(dir: string, shell: "zsh" | "bash"): string {
  const file = shell === "zsh" ? "~/.zshrc" : "~/.bash_profile";
  return `echo 'export PATH="${dir}:$PATH"' >> ${file}`;
}
