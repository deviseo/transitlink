import { useEffect, useState, type ReactNode } from "react";
import { useNavigate } from "react-router";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Check, CircleCheck, ClipboardCopy, LifeBuoy, LoaderCircle, RefreshCw, Stethoscope, Wand2 } from "lucide-react";
import { FindingRow, useFixer, type Fixer } from "./findings";
import { ToolIcon } from "../tools/ToolIcon";
import { Dot } from "../ui/badge";
import { Button } from "../ui/button";
import { Dialog } from "../ui/dialog";
import { getAppInfo } from "../../lib/api";
import { useCurrentSite } from "../../lib/currentSite";
import { errorText } from "../../lib/errors";
import { allFindings, automaticFixes, checkupReport, runCheckup, type Finding } from "../../lib/health";

/** 工单页从这里接过体检报告（路由的 state），打开新建工单时填好。 */
export interface TicketDraft {
  title: string;
  body: string;
}

/**
 * 一键体检：查终端代理、站点和每个用 TransitLink 配置过的工具，问题用大白话说清楚，能修的点一下修好；
 * 修不了的可以复制报告，或者直接带着报告提交工单。
 */
export function CheckupDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const { site } = useCurrentSite();
  const info = useQuery({ queryKey: ["app-info"], queryFn: getAppInfo });
  const checkup = useMutation({ mutationFn: runCheckup });
  const fixer = useFixer(() => checkup.mutate());
  const [fixingAll, setFixingAll] = useState(false);
  const [copied, setCopied] = useState(false);

  const recheck = () => {
    fixer.reset();
    checkup.mutate();
  };

  // 每次打开都重新查。
  useEffect(() => {
    if (open) recheck();
    // checkup 每次渲染都是新对象，只在打开时触发。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const result = checkup.data;
  const findings = result ? allFindings(result) : [];
  const errors = findings.filter((finding) => finding.level === "error").length;
  const warnings = findings.length - errors;
  const fixes = result ? automaticFixes(result).filter((fix) => fixer.state(fix)?.status !== "done") : [];
  const tickets = site?.config?.tickets?.enabled === true;
  const report = () => (result ? checkupReport(t, result, info.data, i18n.language) : "");

  const fixAll = async () => {
    setFixingAll(true);
    await fixer.runAll(fixes);
    setFixingAll(false);
    recheck();
  };

  const copy = async () => {
    await navigator.clipboard.writeText(report());
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  const openTicket = () => {
    const draft: TicketDraft = { title: t("checkup.ticketTitle"), body: report() };
    onClose();
    void navigate("/tickets", { state: { draft } });
  };

  const close = () => {
    if (!fixingAll && !fixer.dialogOpen) onClose();
  };

  return (
    <Dialog open={open} title={t("checkup.title")} onClose={close} closeLabel={t("common.close")} wide>
      {checkup.isPending && (
        <div className="flex flex-col items-center gap-3 py-10 text-center">
          <Stethoscope className="h-8 w-8 animate-pulse text-brand" aria-hidden />
          <p className="text-sm text-muted-foreground">{t("checkup.running")}</p>
        </div>
      )}
      {checkup.isError && <p className="text-sm text-danger">{errorText(t, checkup.error)}</p>}
      {result && !checkup.isPending && (
        <div className="space-y-4">
          <Summary errors={errors} warnings={warnings} fixes={fixes.length} sites={result.sites.length} tools={result.tools.length} fixing={fixingAll} onFixAll={() => void fixAll()} />

          <Section title={t("checkup.sites")}>
            {result.sites.length === 0 && <p className="px-3 py-2.5 text-[13px] text-muted-foreground">{t("checkup.noSites")}</p>}
            {result.sites.map((item) => (
              <Item
                key={item.siteId}
                title={item.name}
                subtitle={item.host}
                facts={[
                  item.latencyMs != null ? t("checkup.latency", { ms: item.latencyMs }) : null,
                  item.balanceUsd != null ? t("checkup.balance", { amount: new Intl.NumberFormat(i18n.language, { style: "currency", currency: "USD" }).format(item.balanceUsd) }) : null,
                ]}
                findings={item.findings}
                fixer={fixer}
              />
            ))}
          </Section>

          <Section title={t("checkup.tools")}>
            {result.tools.length === 0 && <p className="px-3 py-2.5 text-[13px] text-muted-foreground">{t("checkup.noTools")}</p>}
            {result.tools.map((tool) => (
              <Item
                key={tool.kind}
                icon={<ToolIcon kind={tool.kind} size="sm" />}
                title={t(`tools.${tool.kind}`)}
                subtitle={[tool.siteName, tool.model].filter(Boolean).join(" · ")}
                facts={[tool.latencyMs != null ? t("checkup.latency", { ms: tool.latencyMs }) : null]}
                findings={tool.findings}
                fixer={fixer}
              />
            ))}
          </Section>

          <Section title={t("checkup.terminal")}>
            {!result.shellChecked ? (
              <p className="px-3 py-2.5 text-[13px] text-muted-foreground">{t("checkup.shellUnknown")}</p>
            ) : result.proxy ? (
              <Item
                title={t("checkup.proxy", { name: result.proxy.name })}
                subtitle={result.proxy.url}
                facts={[result.proxy.reachable ? t("checkup.proxyUp") : null]}
                findings={result.proxy.findings}
                fixer={fixer}
              />
            ) : (
              <p className="px-3 py-2.5 text-[13px] text-muted-foreground">{t("checkup.noProxy")}</p>
            )}
          </Section>

          <div className="flex flex-wrap items-center justify-end gap-2 pt-1">
            <p className="mr-auto text-xs text-muted-foreground">{t("checkup.noSecrets")}</p>
            <Button variant="secondary" size="sm" onClick={() => void copy()}>
              {copied ? <Check className="h-4 w-4 text-success" aria-hidden /> : <ClipboardCopy className="h-4 w-4" aria-hidden />}
              {copied ? t("common.copied") : t("checkup.copyReport")}
            </Button>
            {tickets && findings.length > 0 && (
              <Button variant="secondary" size="sm" onClick={openTicket}>
                <LifeBuoy className="h-4 w-4" aria-hidden />
                {t("checkup.openTicket")}
              </Button>
            )}
            <Button variant="secondary" size="sm" onClick={recheck} disabled={fixingAll}>
              <RefreshCw className="h-4 w-4" aria-hidden />
              {t("checkup.again")}
            </Button>
          </div>
        </div>
      )}
      {fixer.dialogs}
    </Dialog>
  );
}

function Summary({ errors, warnings, fixes, sites, tools, fixing, onFixAll }: { errors: number; warnings: number; fixes: number; sites: number; tools: number; fixing: boolean; onFixAll: () => void }) {
  const { t } = useTranslation();
  if (errors + warnings === 0) {
    return (
      <div className="flex items-center gap-3 rounded-xl bg-success/10 p-3.5">
        <CircleCheck className="h-6 w-6 shrink-0 text-success" aria-hidden />
        <div>
          <p className="text-sm font-medium text-success">{t("checkup.allGood")}</p>
          <p className="text-xs text-muted-foreground">{t("checkup.checked", { sites, tools })}</p>
        </div>
      </div>
    );
  }
  return (
    <div className={`flex flex-wrap items-center gap-3 rounded-xl p-3.5 ${errors > 0 ? "bg-danger/10" : "bg-warning/10"}`}>
      <div className="min-w-0 flex-1">
        <p className={`text-sm font-medium ${errors > 0 ? "text-danger" : "text-warning"}`}>
          {errors > 0 ? t("checkup.found", { count: errors }) : t("checkup.foundWarnings", { count: warnings })}
          {errors > 0 && warnings > 0 && <span className="font-normal text-muted-foreground">{t("checkup.andWarnings", { count: warnings })}</span>}
        </p>
        <p className="text-xs text-muted-foreground">{fixes > 0 ? t("checkup.canFix", { count: fixes }) : t("checkup.manual")}</p>
      </div>
      {fixes > 0 && (
        <Button size="sm" onClick={onFixAll} disabled={fixing}>
          {fixing ? <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden /> : <Wand2 className="h-4 w-4" aria-hidden />}
          {fixing ? t("checkup.fixing") : t("checkup.fixAll")}
        </Button>
      )}
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section>
      <h3 className="mb-1.5 text-xs font-medium text-muted-foreground">{title}</h3>
      <ul className="divide-y divide-border/70 rounded-xl border border-border/80">{children}</ul>
    </section>
  );
}

function Item({ icon, title, subtitle, facts, findings, fixer }: { icon?: ReactNode; title: string; subtitle?: string; facts: (string | null)[]; findings: Finding[]; fixer: Fixer }) {
  const { t } = useTranslation();
  const tone = findings.some((finding) => finding.level === "error") ? "danger" : findings.length > 0 ? "warning" : "success";
  const shown = facts.filter(Boolean).join(" · ");
  return (
    <li className="px-3 py-2.5">
      <div className="flex items-center gap-2.5">
        {icon}
        <div className="min-w-0 flex-1">
          <p className="truncate text-[13px] font-medium">{title}</p>
          {subtitle && <p className="truncate text-xs text-muted-foreground">{subtitle}</p>}
        </div>
        <span className="flex shrink-0 items-center gap-1.5 text-xs text-muted-foreground tabular-nums">
          {shown}
          <Dot tone={tone} />
          <span className="sr-only">{tone === "success" ? t("checkup.ok") : t("checkup.hasProblems")}</span>
        </span>
      </div>
      {findings.length > 0 && (
        <ul className="mt-2 space-y-1.5">
          {findings.map((finding, index) => (
            <FindingRow key={`${finding.code}-${index}`} finding={finding} fixer={fixer} />
          ))}
        </ul>
      )}
    </li>
  );
}
