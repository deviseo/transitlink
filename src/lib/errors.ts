import type { TFunction } from "i18next";
import { isCommandError } from "./api";

/** 把 Rust 命令的错误转成界面文案；站点自己给了说明（或出错的文件）时附在后面。 */
export function errorText(t: TFunction, error: unknown): string {
  if (isCommandError(error)) {
    const base = t(`errors.${error.code}`, { defaultValue: t("errors.unexpected") });
    return error.message ? t("errors.withMessage", { base, message: error.message }) : base;
  }
  return t("errors.unexpected");
}
