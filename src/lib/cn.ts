import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

/** 合并 className，后面的 Tailwind 类覆盖前面的同类设置。 */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
