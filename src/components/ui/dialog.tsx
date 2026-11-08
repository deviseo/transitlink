import { useEffect, useId, useRef, type ReactNode } from "react";
import { X } from "lucide-react";

/** 打开着的对话框，后打开的在上面。Esc 只关最上面的那个（例如导入窗口里又打开了登录窗口）。 */
const openDialogs: symbol[] = [];

/** 简单的模态框：遮罩、标题、Esc 关闭。 */
export function Dialog({
  open,
  title,
  onClose,
  children,
  closeLabel,
  wide = false,
}: {
  open: boolean;
  title: string;
  onClose: () => void;
  children: ReactNode;
  closeLabel: string;
  /** 内容多（例如一键配置）时用宽一点的窗口。 */
  wide?: boolean;
}) {
  const titleId = useId();
  // onClose 每次渲染可能都是新函数：放进 ref，免得对话框重新排到最上面。
  const close = useRef(onClose);
  useEffect(() => {
    close.current = onClose;
  });
  useEffect(() => {
    if (!open) return;
    const id = Symbol("dialog");
    openDialogs.push(id);
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && openDialogs[openDialogs.length - 1] === id) close.current();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      openDialogs.splice(openDialogs.indexOf(id), 1);
    };
  }, [open]);

  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className={`max-h-[90vh] w-full overflow-y-auto rounded-2xl border border-border/80 bg-card p-5 shadow-[0_24px_64px_-16px_rgba(16,24,40,0.35)] ${wide ? "max-w-xl" : "max-w-md"}`}
      >
        <div className="flex items-center justify-between gap-3">
          <h2 id={titleId} className="text-base font-semibold">
            {title}
          </h2>
          <button type="button" onClick={onClose} aria-label={closeLabel} className="rounded-md p-1 text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand">
            <X className="h-4 w-4" aria-hidden />
          </button>
        </div>
        <div className="mt-4">{children}</div>
      </div>
    </div>
  );
}
