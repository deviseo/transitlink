import type { HTMLAttributes, ReactNode } from "react";
import { cn } from "../../lib/cn";

export function Card({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn("rounded-2xl border border-border/80 bg-card p-5", className)} {...props} />;
}

/** 带标题的卡片：标题、一句说明和右上角的操作，下面是内容。 */
export function SectionCard({
  title,
  description,
  icon,
  action,
  className,
  children,
}: {
  title: string;
  description?: ReactNode;
  icon?: ReactNode;
  action?: ReactNode;
  className?: string;
  children?: ReactNode;
}) {
  return (
    <section className={cn("rounded-2xl border border-border/80 bg-card", className)}>
      <header className="flex items-start justify-between gap-3 px-5 pb-3 pt-4">
        <div className="flex min-w-0 items-start gap-2.5">
          {icon && <span className="mt-0.5 text-muted-foreground">{icon}</span>}
          <div className="min-w-0">
            <h2 className="text-[15px] font-semibold">{title}</h2>
            {description && <p className="mt-0.5 text-xs text-muted-foreground">{description}</p>}
          </div>
        </div>
        {action && <div className="shrink-0">{action}</div>}
      </header>
      {children && <div className="px-5 pb-5">{children}</div>}
    </section>
  );
}
