import { useEffect, useRef, useState, type ClipboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { ImageOff, ImagePlus, LoaderCircle, X } from "lucide-react";
import { Button } from "../ui/button";
import { ticketAttachment, type TicketAttachment, type TicketSession } from "../../lib/api";
import { errorText } from "../../lib/errors";
import { IMAGE_TYPES, readImage, ticketKeys, type PickedImage } from "../../lib/tickets";

const megabytes = (bytes: number) => Math.round((bytes / 1024 / 1024) * 10) / 10;

/** 要随消息发出的图片：选文件或在输入框里粘贴截图；按站长给的数量、大小限制和格式检查。 */
export function useImageAttachments(limits: TicketSession["limits"] | undefined) {
  const { t } = useTranslation();
  const [images, setImages] = useState<PickedImage[]>([]);
  const [error, setError] = useState<string | null>(null);
  const enabled = limits?.enabled ?? false;
  const max = limits?.maxImages ?? 5;
  const maxBytes = limits?.maxImageBytes ?? 5 * 1024 * 1024;

  const add = async (files: File[]) => {
    if (!enabled || files.length === 0) return;
    const problems: string[] = [];
    const accepted = files.filter((file) => {
      if (!IMAGE_TYPES.includes(file.type)) {
        problems.push(t("tickets.imageInvalid", { name: file.name }));
        return false;
      }
      if (file.size > maxBytes) {
        problems.push(t("tickets.imageTooLarge", { name: file.name, size: megabytes(maxBytes) }));
        return false;
      }
      return true;
    });
    const room = Math.max(0, max - images.length);
    if (accepted.length > room) problems.push(t("tickets.imageTooMany", { max }));
    try {
      const read = await Promise.all(accepted.slice(0, room).map(readImage));
      setImages((current) => [...current, ...read].slice(0, max));
    } catch {
      problems.push(t("tickets.imageReadFailed"));
    }
    setError(problems.length > 0 ? problems.join(t("common.separator")) : null);
  };

  return {
    enabled,
    max,
    maxBytes,
    images,
    error,
    add,
    remove: (key: string) => setImages((current) => current.filter((image) => image.key !== key)),
    clear: () => {
      setImages([]);
      setError(null);
    },
    /** 在输入框里粘贴截图。 */
    onPaste: (event: ClipboardEvent<HTMLElement>) => {
      const files = Array.from(event.clipboardData.files).filter((file) => file.type.startsWith("image/"));
      if (files.length > 0) void add(files);
    },
  };
}

export type ImageAttachments = ReturnType<typeof useImageAttachments>;

/** 选好的图片（缩略图，可以移除）和「添加图片」按钮。站长没开图片上传时不显示。 */
export function ImageStrip({ attachments, disabled = false }: { attachments: ImageAttachments; disabled?: boolean }) {
  const { t } = useTranslation();
  const input = useRef<HTMLInputElement>(null);
  if (!attachments.enabled) return null;
  const full = attachments.images.length >= attachments.max;
  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        {attachments.images.map((image) => (
          <div key={image.key} className="relative h-16 w-16 overflow-hidden rounded-lg border border-border bg-muted">
            <img src={image.dataUrl} alt={image.name} title={image.name} className="h-full w-full object-cover" />
            <button
              type="button"
              onClick={() => attachments.remove(image.key)}
              disabled={disabled}
              aria-label={t("tickets.removeImage", { name: image.name })}
              className="absolute right-0.5 top-0.5 rounded-full bg-black/60 p-0.5 text-white hover:bg-black/80 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand disabled:opacity-50"
            >
              <X className="h-3 w-3" aria-hidden />
            </button>
          </div>
        ))}
        <Button variant="secondary" size="sm" onClick={() => input.current?.click()} disabled={disabled || full}>
          <ImagePlus className="h-4 w-4" aria-hidden />
          {t("tickets.addImage", { count: attachments.images.length, max: attachments.max })}
        </Button>
        <input
          ref={input}
          type="file"
          accept={IMAGE_TYPES.join(",")}
          multiple
          hidden
          onChange={(event) => {
            void attachments.add(Array.from(event.target.files ?? []));
            // 清空，同一张图片移除后还能再选。
            event.target.value = "";
          }}
        />
      </div>
      <p className="text-xs text-muted-foreground">{t("tickets.imagesHint", { max: attachments.max, size: megabytes(attachments.maxBytes) })}</p>
      {attachments.error && (
        <p className="text-xs text-danger" role="alert">
          {attachments.error}
        </p>
      )}
    </div>
  );
}

export interface ViewedImage {
  src: string;
  name: string;
}

/** 消息里的一张图片：缩略图，点开看大图。图片由桌面端取回（要带工单会话），取过的留在缓存里。 */
export function AttachmentThumb({ siteId, attachment, onOpen }: { siteId: string; attachment: TicketAttachment; onOpen: (image: ViewedImage) => void }) {
  const { t } = useTranslation();
  const image = useQuery({
    queryKey: ticketKeys.attachment(siteId, attachment.id),
    queryFn: () => ticketAttachment(siteId, attachment.id),
    staleTime: Infinity,
    gcTime: 30 * 60 * 1000,
  });
  return (
    <button
      type="button"
      onClick={() => image.data && onOpen({ src: image.data, name: attachment.originalName })}
      disabled={!image.data}
      title={image.isError ? errorText(t, image.error) : attachment.originalName}
      aria-label={t("tickets.viewImage", { name: attachment.originalName })}
      className="flex h-20 w-32 items-center justify-center overflow-hidden rounded-lg border border-border bg-muted transition-opacity hover:opacity-90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand disabled:cursor-default"
    >
      {image.data ? (
        <img src={image.data} alt="" className="h-full w-full object-cover" />
      ) : image.isError ? (
        <ImageOff className="h-5 w-5 text-muted-foreground" aria-hidden />
      ) : (
        <LoaderCircle className="h-4 w-4 animate-spin text-muted-foreground" aria-hidden />
      )}
    </button>
  );
}

/** 看大图：点遮罩、关闭按钮或按 Esc 关闭。 */
export function ImageViewer({ image, onClose }: { image: ViewedImage | null; onClose: () => void }) {
  const { t } = useTranslation();
  useEffect(() => {
    if (!image) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [image, onClose]);

  if (!image) return null;
  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={image.name}
      className="fixed inset-0 z-50 flex flex-col items-center justify-center gap-3 bg-black/80 p-10"
      onMouseDown={(event) => event.target === event.currentTarget && onClose()}
    >
      <button
        type="button"
        onClick={onClose}
        aria-label={t("common.close")}
        className="absolute right-4 top-4 rounded-full bg-white/10 p-2 text-white hover:bg-white/20 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white"
      >
        <X className="h-5 w-5" aria-hidden />
      </button>
      <img src={image.src} alt={image.name} className="max-h-[80vh] max-w-full rounded-lg object-contain shadow-2xl" />
      <p className="max-w-full truncate text-xs text-white/80">{image.name}</p>
    </div>
  );
}
