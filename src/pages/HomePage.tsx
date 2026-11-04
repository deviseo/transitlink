import { Link } from "react-router";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { ArrowRight, KeyRound, Link2, Sparkles, Terminal } from "lucide-react";
import { buttonVariants } from "../components/ui/button";
import { Card } from "../components/ui/card";
import { detectTools } from "../lib/api";

export function HomePage() {
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
