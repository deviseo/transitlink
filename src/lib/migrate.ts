import { call } from "./tauri";
import type { SwitchReport, ToolKind } from "./api";

/** 导入前给用户看的：有哪些站点（这台电脑上已经有的会合并）、哪些工具要恢复。 */
export interface BackupPreview {
  exportedAt: number;
  sites: { name: string; host: string; account: string; exists: boolean }[];
  tools: { kind: ToolKind; siteName: string; configured: boolean }[];
  /** 认不出来、跳过了的站点和工具（新版本导出的）。 */
  skipped: number;
}

/** 等着恢复的工具：登录站点后一键用原来的分组和模型配置好。 */
export interface PendingTool {
  kind: ToolKind;
  siteId: string;
  siteName: string;
}

export interface ImportOutcome {
  added: number;
  merged: number;
  pending: PendingTool[];
}

const pause = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));
let previewPending: PendingTool[] = [];

/** 导出到「下载」文件夹（不含登录凭证和 Key），返回文件位置。 */
export const exportBackup = () =>
  call<string>("export_backup", async () => {
    await pause(400);
    return `~/Downloads/transitlink-backup-${new Date().toISOString().slice(0, 10)}.json`;
  });

/** 看看备份里有什么。预览时不管选了什么文件，都当作有一个站点、两个工具的备份。 */
export const previewBackup = (text: string) =>
  call<BackupPreview>("preview_backup", () => {
    if (!text.includes("transitlink-backup")) throw { code: "backupInvalid", message: null };
    return {
      exportedAt: Math.floor(Date.now() / 1000) - 3600,
      sites: [{ name: "示例中转站", host: "relay.example.com", account: "demo", exists: false }],
      tools: [
        { kind: "claudeCode", siteName: "示例中转站", configured: false },
        { kind: "codex", siteName: "示例中转站", configured: false },
      ],
      skipped: 0,
    };
  }, { text });

/** 导入备份。 */
export const importBackup = (text: string) =>
  call<ImportOutcome>("import_backup", async () => {
    await pause(500);
    previewPending = [
      { kind: "claudeCode", siteId: "preview", siteName: "示例中转站" },
      { kind: "codex", siteId: "preview", siteName: "示例中转站" },
    ];
    return { added: 1, merged: 0, pending: [...previewPending] };
  }, { text });

/** 从备份导入、还没恢复的工具。 */
export const pendingRestore = () => call<PendingTool[]>("pending_restore", () => [...previewPending]);

/** 一键恢复这个站点上的工具。 */
export const restorePending = (siteId: string) =>
  call<SwitchReport>("restore_pending", async () => {
    await pause(800);
    const switched = previewPending.filter((tool) => tool.siteId === siteId).map((tool) => tool.kind);
    previewPending = previewPending.filter((tool) => tool.siteId !== siteId);
    return { siteId, switched, failures: [] };
  }, { siteId });

/** 不恢复了。 */
export const dismissPending = () =>
  call<null>("dismiss_pending", () => {
    previewPending = [];
    return null;
  });
