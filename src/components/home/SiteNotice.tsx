import { useState } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Check, Copy, ExternalLink, Headset, Megaphone } from "lucide-react";
import { Button } from "../ui/button";
import { Card } from "../ui/card";
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

/** 首页：站长的欢迎语、公告和客服方式（用专属链接接入的站点才有）。 */
export function SiteNotice({ brand }: { brand: ActiveBrand }) {
  const { t } = useTranslation();
  const config = brand.site?.config;
  if (!config || (!config.notice.content && config.support.length === 0)) return null;
  return (
    <Card className="space-y-4">
      {config.notice.content && (
        <div className="flex gap-3">
          <Megaphone className="mt-0.5 h-5 w-5 shrink-0 text-brand" aria-hidden />
          <div className="min-w-0">
            <h2 className="font-medium">{t("home.notice")}</h2>
            <p className="mt-1 whitespace-pre-line text-sm text-muted-foreground">{config.notice.content}</p>
          </div>
        </div>
      )}
      {config.support.length > 0 && (
        <div className="flex gap-3">
          <Headset className="mt-0.5 h-5 w-5 shrink-0 text-brand" aria-hidden />
          <div className="min-w-0 flex-1">
            <h2 className="font-medium">{t("home.support")}</h2>
            <ul className="mt-2 flex flex-wrap gap-2">
              {config.support.map((item) => (
                <SupportItem key={`${item.type}-${item.value}`} item={item} />
              ))}
            </ul>
          </div>
        </div>
      )}
    </Card>
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
