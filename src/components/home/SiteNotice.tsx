import { useState } from "react";
import { Link } from "react-router";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Check, Copy, ExternalLink, Headset, LifeBuoy, Megaphone } from "lucide-react";
import { Button, buttonVariants } from "../ui/button";
import { SectionCard } from "../ui/card";
import { inDesktop } from "../../lib/tauri";
import type { ActiveBrand } from "../../lib/useActiveBrand";
import type { SiteConfig } from "../../lib/api";

type Support = SiteConfig["support"][number];

/** 能直接打开的客服方式：网页链接和邮箱。QQ、微信号这类只能复制。 */
function supportTarget(item: Support): string | null {
  const value = item.value.trim();
  if (/^https?:\/\//i.test(value)) return value;
  if (item.type === "email" || /^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(value)) return `mailto:${value}`;
  return null;
}

/** 首页：站长的欢迎语、公告和客服方式（用专属链接接入的站点才有）；站长开了工单时可以直接提交工单。 */
export function SiteNotice({ brand }: { brand: ActiveBrand }) {
  const { t } = useTranslation();
  const config = brand.site?.config;
  const tickets = config?.tickets?.enabled ?? false;
  if (!brand.site || !config || (!config.notice.content && config.support.length === 0 && !tickets)) return null;
  const notice = config.notice.content;
  const support = config.support.length > 0 || tickets;
  return (
    <div className={`grid gap-4 ${notice && support ? "md:grid-cols-2" : ""}`}>
      {notice && (
        <SectionCard title={t("home.notice")} icon={<Megaphone className="h-4 w-4 text-brand" aria-hidden />}>
          <p className="whitespace-pre-line text-[13px] leading-relaxed text-muted-foreground">{notice}</p>
        </SectionCard>
      )}
      {support && (
        <SectionCard title={t("home.support")} icon={<Headset className="h-4 w-4 text-brand" aria-hidden />}>
          <ul className="flex flex-wrap gap-2">
            {tickets && (
              <li>
                <Link to="/tickets" className={buttonVariants({ size: "sm" })}>
                  <LifeBuoy className="h-4 w-4" aria-hidden />
                  {t("home.openTicket")}
                </Link>
              </li>
            )}
            {config.support.map((item) => (
              <SupportItem key={`${item.type}-${item.value}`} item={item} />
            ))}
          </ul>
        </SectionCard>
      )}
    </div>
  );
}

function SupportItem({ item }: { item: Support }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const target = supportTarget(item);
  const label = item.label || item.value;
  if (target) {
    const open = () => (inDesktop ? void openUrl(target) : void window.open(target, "_blank", "noopener"));
    return (
      <li>
        <Button variant="secondary" size="sm" onClick={open}>
          <ExternalLink className="h-4 w-4" aria-hidden />
          {label}
        </Button>
      </li>
    );
  }
  const copy = async () => {
    await navigator.clipboard.writeText(item.value);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };
  return (
    <li>
      <Button variant="secondary" size="sm" onClick={() => void copy()} title={t("home.copyContact")}>
        {copied ? <Check className="h-4 w-4 text-success" aria-hidden /> : <Copy className="h-4 w-4" aria-hidden />}
        {item.label ? t("home.contact", { label: item.label, value: item.value }) : item.value}
      </Button>
    </li>
  );
}
