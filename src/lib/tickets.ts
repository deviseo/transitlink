import { useEffect } from "react";
import { queryOptions, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { listSites, listTickets, type ImageUpload, type SiteView, type TicketDetail, type TicketList, type TicketSummary } from "./api";

/** 工单列表（未读数）和正在看的工单多久刷新一次。 */
const REFRESH_MS = 5 * 60 * 1000;
/** 窗口回到前台时，超过 30 秒没更新的才重新读取。 */
const STALE_MS = 30 * 1000;

export const ticketKeys = {
  all: ["tickets"] as const,
  list: (siteId: string) => ["tickets", siteId, "list"] as const,
  detail: (siteId: string, ticketId: string) => ["tickets", siteId, "detail", ticketId] as const,
  session: (siteId: string) => ["ticket-session", siteId] as const,
  attachment: (siteId: string, attachmentId: string) => ["ticket-attachment", siteId, attachmentId] as const,
};

/** 一个站点的工单列表：每 5 分钟刷新一次（窗口回到前台时也刷新，见 useTicketFocusRefresh）。 */
export const ticketListQuery = (siteId: string) =>
  queryOptions({ queryKey: ticketKeys.list(siteId), queryFn: () => listTickets(siteId), refetchInterval: REFRESH_MS, staleTime: STALE_MS });

export const ticketDetailOptions = { refetchInterval: REFRESH_MS, staleTime: STALE_MS };

/**
 * 开了工单的站点（站长配置里打开了 tickets）。每个站点的工单是分开的（各自的站长和客服）：
 * 工单页只看当前站点的，左上角的切换器显示其他站点有没有新回复。`loaded`：站点列表读完了。
 */
export function useTicketSites(): { sites: SiteView[]; loaded: boolean } {
  const query = useQuery({ queryKey: ["sites"], queryFn: listSites });
  const sites = (query.data ?? []).filter((site) => site.config?.tickets?.enabled);
  return { sites, loaded: query.data !== undefined };
}

/** 各站点有新回复的工单数（只算用户同意过、读过列表的站点）。 */
export function useTicketUnread(): Map<string, number> {
  const { sites } = useTicketSites();
  const lists = useTicketLists(sites);
  return new Map(sites.map((site, index) => [site.id, lists[index]?.data?.unread ?? 0]));
}

/**
 * 每个开了工单的站点的工单列表（和工单页共用缓存）。用户还没同意的站点不读取（结果和 `sites` 一一对应，
 * 没同意的没有数据）：读取要把站点的登录交给 TransitHub，启动时不能自己就发出去。
 */
export function useTicketLists(sites: SiteView[]) {
  return useQueries({ queries: sites.map((site) => ({ ...ticketListQuery(site.id), enabled: site.ticketsAllowed })) });
}

/** 窗口回到前台：重新读取工单列表和正在看的工单。 */
export function useTicketFocusRefresh() {
  const queryClient = useQueryClient();
  useEffect(() => {
    const onFocus = () => void queryClient.refetchQueries({ queryKey: ticketKeys.all, type: "active", stale: true });
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [queryClient]);
}

/** 列表里的那一项。 */
export function summaryOf(ticket: TicketDetail): TicketSummary {
  const { id, title, category, status, contact, unread, lastMessageAt, createdAt } = ticket;
  return { id, title, category, status, contact, unread, lastMessageAt, createdAt };
}

/** 打开、回复或关闭工单后，用最新的详情更新列表（状态、时间、已读），重新算未读数。 */
export function syncList(list: TicketList | undefined, ticket: TicketDetail): TicketList | undefined {
  if (!list) return list;
  const others = list.items.filter((item) => item.id !== ticket.id);
  const items = [summaryOf(ticket), ...others].sort((a, b) => parseTime(b.lastMessageAt) - parseTime(a.lastMessageAt));
  return { items, unread: items.filter((item) => item.unread).length };
}

/** 解析 TransitHub 的时间（Go 的时间可能带到微秒，只保留到毫秒，各家浏览器都能解析）。 */
function parseTime(value: string): number {
  return Date.parse(value.replace(/(\.\d{3})\d+/, "$1"));
}

/** 「3 分钟前」「昨天」这样的相对时间；一周以前的显示日期。 */
export function relativeTime(value: string, language: string, now = Date.now()): string {
  const time = parseTime(value);
  if (Number.isNaN(time)) return "";
  const seconds = (time - now) / 1000;
  const abs = Math.abs(seconds);
  const format = new Intl.RelativeTimeFormat(language, { numeric: "auto" });
  if (abs < 60) return format.format(0, "second");
  if (abs < 3600) return format.format(Math.round(seconds / 60), "minute");
  if (abs < 86_400) return format.format(Math.round(seconds / 3600), "hour");
  if (abs < 7 * 86_400) return format.format(Math.round(seconds / 86_400), "day");
  return new Intl.DateTimeFormat(language, { dateStyle: "medium" }).format(time);
}

/** 消息的时间：日期和几点几分。 */
export function messageTime(value: string, language: string): string {
  const time = parseTime(value);
  if (Number.isNaN(time)) return "";
  return new Intl.DateTimeFormat(language, { dateStyle: "medium", timeStyle: "short" }).format(time);
}

/** 能上传的图片格式（和 TransitHub 一致）。 */
export const IMAGE_TYPES = ["image/png", "image/jpeg", "image/webp", "image/gif"];

/** 选好、还没发出去的图片。 */
export interface PickedImage {
  key: string;
  name: string;
  size: number;
  /** `data:` 地址，用来预览，发送时取出 base64。 */
  dataUrl: string;
}

let nextImageKey = 0;

export function readImage(file: File): Promise<PickedImage> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      nextImageKey += 1;
      resolve({ key: String(nextImageKey), name: file.name || "image.png", size: file.size, dataUrl: String(reader.result) });
    };
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

export const toUploads = (images: PickedImage[]): ImageUpload[] =>
  images.map((image) => ({ name: image.name, dataBase64: image.dataUrl.slice(image.dataUrl.indexOf(",") + 1) }));
