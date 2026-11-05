import { Link } from "react-router";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { ArrowRight, KeyRound, Link2, LoaderCircle, Sparkles, Terminal } from "lucide-react";
import { SiteNotice } from "../components/home/SiteNotice";
import { ToolsSummary } from "../components/home/ToolsSummary";
import { BrandMark } from "../components/layout/BrandMark";
import { ReloginNotice } from "../components/sites/ReloginNotice";
import { SiteAvatar } from "../components/sites/SiteAvatar";
import { buttonVariants } from "../components/ui/button";
import { Card } from "../components/ui/card";
import { detectTools, isCommandError, siteAccount, type SiteView } from "../lib/api";
import { siteHost, siteName, useCurrentSite } from "../lib/currentSite";
import { errorText } from "../lib/errors";
import { useActiveBrand } from "../lib/useActiveBrand";

/** 概览：当前站点的余额、编程工具、公告和客服；还没有站点时是三步上手引导。 */
export function HomePage() {
  const { site, loaded } = useCurrentSite();
  if (!loaded) return <LoaderCircle className="h-5 w-5 animate-spin text-muted-foreground" aria-hidden />;
  return site ? <Overview site={site} /> : <GettingStarted />;
}

function Overview({ site }: { site: SiteView }) {
  const { t, i18n } = useTranslation();
  const brand = useActiveBrand();
  const account = useQuery({ queryKey: ["site-account", site.id], queryFn: () => siteAccount(site.id) });
  const expired = isCommandError(account.error) && account.error.code === "sessionExpired";
  const money = new Intl.NumberFormat(i18n.language, { style: "currency", currency: "USD", maximumFractionDigits: 2 });

  return (
    <div className="space-y-6">
      <header className="flex flex-wrap items-center gap-4">
        {brand.site ? <BrandMark brand={brand} className="h-12 w-12" /> : <SiteAvatar name={site.name} logoUrl={site.logoUrl} className="h-12 w-12" />}
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-2xl font-semibold">{siteName(site)}</h1>
          <p className="truncate text-sm text-muted-foreground">
            {site.account} · {siteHost(site)}
          </p>
        </div>
        <div className="text-right">
          <p className="text-xs text-muted-foreground">{t("overview.balance")}</p>
          {account.isLoading && <LoaderCircle className="ml-auto mt-1 h-5 w-5 animate-spin text-muted-foreground" aria-hidden />}
          {account.data && <p className="text-2xl font-semibold tabular-nums">{money.format(account.data.balanceUsd)}</p>}
          {account.isError && !expired && <p className="max-w-56 text-xs text-danger">{errorText(t, account.error)}</p>}
        </div>
      </header>

      {brand.welcome && <p className="text-sm text-muted-foreground">{brand.welcome}</p>}
      <ReloginNotice site={site} />
      <ToolsSummary site={site} />
      <SiteNotice brand={brand} />
    </div>
  );
}

/** 还没有站点：三步上手（接入站点、登录、一键配置）。 */
function GettingStarted() {
  const { t } = useTranslation();
  const { data: tools } = useQuery({ queryKey: ["tools"], queryFn: detectTools });
  const installed = (tools ?? []).filter((tool) => tool.installed).map((tool) => t(`tools.${tool.kind}`));
  const steps = [
    { title: t("home.step1"), description: t("home.step1Desc"), icon: Link2 },
    { title: t("home.step2"), description: t("home.step2Desc"), icon: KeyRound },
    { title: t("home.step3"), description: t("home.step3Desc"), icon: Sparkles },
  ];

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold">{t("home.title")}</h1>
        <p className="mt-1 text-sm text-muted-foreground">{t("home.subtitle")}</p>
      </header>

      <ol className="grid gap-3 md:grid-cols-3">
        {steps.map((step, index) => (
          <li key={step.title}>
            <Card className="h-full">
              <div className="flex items-center gap-2">
                <span className="flex h-7 w-7 items-center justify-center rounded-full bg-brand/10 text-sm font-semibold text-brand">{index + 1}</span>
                <step.icon className="h-4 w-4 text-muted-foreground" aria-hidden />
              </div>
              <h2 className="mt-3 font-medium">{step.title}</h2>
              <p className="mt-1 text-sm text-muted-foreground">{step.description}</p>
            </Card>
          </li>
        ))}
      </ol>

      <Card className="flex flex-wrap items-center justify-between gap-4">
        <div className="flex items-center gap-3">
          <Terminal className="h-5 w-5 text-muted-foreground" aria-hidden />
          <p className="text-sm">
            {tools && (installed.length > 0 ? t("home.toolsDetected", { names: installed.join(t("common.separator")) }) : t("home.toolsMissing"))}
          </p>
        </div>
        <div className="flex gap-2">
          <Link to="/tools" className={buttonVariants({ variant: "secondary", size: "sm" })}>
            {t("home.goTools")}
          </Link>
          <Link to="/sites" className={buttonVariants({ size: "sm" })}>
            {t("home.goSites")}
            <ArrowRight className="h-4 w-4" aria-hidden />
          </Link>
        </div>
      </Card>
    </div>
  );
}
