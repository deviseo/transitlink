import { invoke, isTauri } from "@tauri-apps/api/core";

/** 是否运行在 TransitLink 桌面端里（用浏览器打开前端时为 false，用示例数据预览界面）。 */
export const inDesktop = isTauri();

/** 调用 Rust 命令；在浏览器预览时用 preview 提供的示例数据。 */
export async function call<T>(command: string, preview: () => T | Promise<T>, args?: Record<string, unknown>): Promise<T> {
  if (!inDesktop) return preview();
  return invoke<T>(command, args);
}
