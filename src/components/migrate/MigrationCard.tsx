import { useRef, useState, type ChangeEvent } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CircleCheck, Download, FolderOpen, LoaderCircle, MonitorSmartphone, RotateCcw, Upload } from "lucide-react";
import { AddSiteDialog } from "../sites/AddSiteDialog";
import { ToolIcon } from "../tools/ToolIcon";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { SectionCard } from "../ui/card";
import { Dialog } from "../ui/dialog";
import { isCommandError, siteAccount, type SwitchReport } from "../../lib/api";
import { settingsKey, useCurrentSite } from "../../lib/currentSite";
import { errorText } from "../../lib/errors";
import { dismissPending, exportBackup, importBackup, pendingRestore, previewBackup, restorePending, type BackupPreview, type ImportOutcome, type PendingTool } from "../../lib/migrate";

/** 和桌面端一样：备份文件最大 4 MB。 */
const MAX_BYTES = 4 * 1024 * 1024;

/** 设置里的「换电脑」：导出到「下载」文件夹；在新电脑上导入，登录站点后一键恢复工具配置。 */
export function MigrationCard() {
  const { t } = useTranslation();
  const input = useRef<HTMLInputElement>(null);
  const pending = useQuery({ queryKey: ["pending-restore"], queryFn: pendingRestore });
  const exported = useMutation({ mutationFn: exportBackup });
  const [picked, setPicked] = useState<{ text: string; preview: BackupPreview } | null>(null);
  const [readError, setReadError] = useState<unknown>(null);

  const onPick = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    setReadError(null);
    try {
      if (file.size > MAX_BYTES) throw { code: "backupTooLarge", message: null };
      const text = await file.text();
      setPicked({ text, preview: await previewBackup(text) });
    } catch (error) {
      setReadError(error);
    }
  };

  return (
    <SectionCard title={t("migrate.title")} description={t("migrate.description")} icon={<MonitorSmartphone className="h-4 w-4" aria-hidden />}>
      <div className="flex flex-wrap gap-2">
        <Button variant="secondary" size="sm" onClick={() => exported.mutate()} disabled={exported.isPending}>
          {exported.isPending ? <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden /> : <Download className="h-4 w-4" aria-hidden />}
          {t("migrate.export")}
        </Button>
        <Button variant="secondary" size="sm" onClick={() => input.current?.click()}>
          <Upload className="h-4 w-4" aria-hidden />
          {t("migrate.import")}
        </Button>
        <input ref={input} type="file" accept=".json,application/json" className="hidden" onChange={(event) => void onPick(event)} />
      </div>
      <p className="mt-2 text-xs text-muted-foreground">{t("migrate.privacy")}</p>
      {exported.isSuccess && (
        <p className="mt-2 flex items-start gap-1.5 text-[13px] text-success">
          <FolderOpen className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
          <span className="min-w-0 break-all">{t("migrate.exported", { path: exported.data })}</span>
        </p>
      )}
      {exported.isError && <p className="mt-2 text-[13px] text-danger">{errorText(t, exported.error)}</p>}
      {readError != null && <p className="mt-2 text-[13px] text-danger">{errorText(t, readError)}</p>}
      {pending.data && pending.data.length > 0 && (
        <div className="mt-4 border-t border-border/70 pt-4">
          <PendingRestore pending={pending.data} />
        </div>
      )}
      {picked && <ImportDialog text={picked.text} preview={picked.preview} onClose={() => setPicked(null)} />}
    </SectionCard>
  );
}

function ImportDialog({ text, preview, onClose }: { text: string; preview: BackupPreview; onClose: () => void }) {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const run = useMutation({
    mutationFn: () => importBackup(text),
    onSuccess: (outcome: ImportOutcome) => {
      queryClient.setQueryData(["pending-restore"], outcome.pending);
      void queryClient.invalidateQueries({ queryKey: ["sites"] });
      void queryClient.invalidateQueries({ queryKey: settingsKey });
    },
  });
  const outcome = run.data;

  return (
    <Dialog open title={t("migrate.importTitle")} onClose={() => !run.isPending && onClose()} closeLabel={t("common.close")} wide>
      {!outcome ? (
        <div className="space-y-4">
          <p className="text-[13px] text-muted-foreground">{t("migrate.exportedAt", { time: new Date(preview.exportedAt * 1000).toLocaleString(i18n.language) })}</p>
          <div>
            <h3 className="mb-1.5 text-xs font-medium text-muted-foreground">{t("migrate.sites", { count: preview.sites.length })}</h3>
            <ul className="divide-y divide-border/70 rounded-xl border border-border/80">
              {preview.sites.map((site) => (
                <li key={`${site.host}-${site.account}`} className="flex items-center gap-3 px-3 py-2.5 text-[13px]">
                  <span className="min-w-0 flex-1">
                    <span className="block truncate font-medium">{site.name}</span>
                    <span className="block truncate text-xs text-muted-foreground">
                      {site.account} · {site.host}
                    </span>
                  </span>
                  {site.exists && <Badge>{t("migrate.siteExists")}</Badge>}
                </li>
              ))}
            </ul>
          </div>
          {preview.tools.length > 0 && (
            <div>
              <h3 className="mb-1.5 text-xs font-medium text-muted-foreground">{t("migrate.tools")}</h3>
              <ul className="divide-y divide-border/70 rounded-xl border border-border/80">
                {preview.tools.map((tool) => (
                  <li key={tool.kind} className="flex items-center gap-2.5 px-3 py-2 text-[13px]">
                    <ToolIcon kind={tool.kind} size="sm" />
                    <span className="min-w-0 flex-1 truncate">
                      {t(`tools.${tool.kind}`)} → {tool.siteName}
                    </span>
                    {tool.configured && <Badge>{t("migrate.toolConfigured")}</Badge>}
                  </li>
                ))}
              </ul>
            </div>
          )}
          {preview.skipped > 0 && <p className="text-xs text-warning">{t("migrate.skipped", { count: preview.skipped })}</p>}
          <p className="text-xs leading-5 text-muted-foreground">{t("migrate.importExplain")}</p>
          {run.isError && <p className="text-[13px] text-danger">{errorText(t, run.error)}</p>}
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={onClose} disabled={run.isPending}>
              {t("common.cancel")}
            </Button>
            <Button onClick={() => run.mutate()} disabled={run.isPending || preview.sites.length === 0}>
              {run.isPending && <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />}
              {t("migrate.importNow")}
            </Button>
          </div>
        </div>
      ) : (
        <div className="space-y-4">
          <p className="flex items-center gap-2 rounded-xl bg-success/10 p-3 text-[13px] text-success">
            <CircleCheck className="h-5 w-5 shrink-0" aria-hidden />
            {outcome.merged > 0 ? t("migrate.importedMerged", { added: outcome.added, merged: outcome.merged }) : t("migrate.imported", { count: outcome.added })}
          </p>
          {outcome.pending.length > 0 ? <PendingRestore pending={outcome.pending} /> : <p className="text-[13px] text-muted-foreground">{t("migrate.nextLogin")}</p>}
          <div className="flex justify-end">
            <Button onClick={onClose}>{t("setup.finish")}</Button>
          </div>
        </div>
      )}
    </Dialog>
  );
}

/** 等着恢复的工具，按站点分组：没登录的先登录，登录了的一键恢复。`title`：换个标题（概览上单独成卡片时）。 */
export function PendingRestore({ pending, title }: { pending: PendingTool[]; title?: string }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const dismiss = useMutation({
    mutationFn: dismissPending,
    onSuccess: () => queryClient.setQueryData(["pending-restore"], []),
  });
  const sites = [...new Set(pending.map((tool) => tool.siteId))];
  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-3">
        {title ? <h2 className="text-[15px] font-semibold">{title}</h2> : <p className="text-[13px] font-medium">{t("migrate.pendingTitle")}</p>}
        <Button variant="ghost" size="sm" className="h-7 px-2 text-xs" onClick={() => dismiss.mutate()} disabled={dismiss.isPending}>
          {t("migrate.dismiss")}
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">{t("migrate.pendingExplain")}</p>
      <ul className="divide-y divide-border/70 rounded-xl border border-border/80">
        {sites.map((siteId) => (
          <PendingSite key={siteId} siteId={siteId} tools={pending.filter((tool) => tool.siteId === siteId)} />
        ))}
      </ul>
    </div>
  );
}

function PendingSite({ siteId, tools }: { siteId: string; tools: PendingTool[] }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { sites } = useCurrentSite();
  const site = sites.find((item) => item.id === siteId);
  const account = useQuery({ queryKey: ["site-account", siteId], queryFn: () => siteAccount(siteId), retry: false });
  const expired = isCommandError(account.error) && account.error.code === "sessionExpired";
  const [login, setLogin] = useState(false);
  const restore = useMutation({
    mutationFn: () => restorePending(siteId),
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: ["pending-restore"] });
      void queryClient.invalidateQueries({ queryKey: ["tools"] });
      void queryClient.invalidateQueries({ queryKey: ["site-keys", siteId] });
    },
  });
  const report: SwitchReport | undefined = restore.data;

  return (
    <li className="space-y-2 px-3 py-2.5">
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <p className="truncate text-[13px] font-medium">{tools[0].siteName}</p>
          <p className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
            {tools.map((tool) => (
              <span key={tool.kind} className="inline-flex items-center gap-1">
                <ToolIcon kind={tool.kind} size="xs" />
                {t(`tools.${tool.kind}`)}
              </span>
            ))}
          </p>
        </div>
        {account.isLoading ? (
          <LoaderCircle className="h-4 w-4 animate-spin text-muted-foreground" aria-hidden />
        ) : expired ? (
          <Button size="sm" variant="secondary" onClick={() => setLogin(true)} disabled={!site}>
            {t("migrate.login")}
          </Button>
        ) : (
          <Button size="sm" onClick={() => restore.mutate()} disabled={restore.isPending || account.isError}>
            {restore.isPending ? <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden /> : <RotateCcw className="h-4 w-4" aria-hidden />}
            {t("migrate.restore")}
          </Button>
        )}
      </div>
      {expired && <p className="text-xs text-muted-foreground">{t("migrate.needLogin")}</p>}
      {account.isError && !expired && <p className="text-xs text-danger">{errorText(t, account.error)}</p>}
      {restore.isError && <p className="text-xs text-danger">{errorText(t, restore.error)}</p>}
      {report && report.failures.length > 0 && (
        <ul className="space-y-0.5 text-xs text-danger">
          {report.failures.map((failure) => (
            <li key={failure.kind}>{t("tray.failure", { tool: t(`tools.${failure.kind}`), error: errorText(t, failure.error) })}</li>
          ))}
        </ul>
      )}
      {login && site && (
        <AddSiteDialog
          open
          initialUrl={site.baseUrl}
          initialAccount={site.account}
          onClose={() => setLogin(false)}
          onAdded={() => {
            setLogin(false);
            void queryClient.invalidateQueries({ queryKey: ["sites"] });
            void queryClient.invalidateQueries({ queryKey: ["site-account", siteId] });
          }}
        />
      )}
    </li>
  );
}
