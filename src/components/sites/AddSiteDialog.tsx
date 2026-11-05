import { useEffect, useState, type SubmitEvent } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ExternalLink, LoaderCircle, ShieldAlert, ShieldCheck } from "lucide-react";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Dialog } from "../ui/dialog";
import { Field, Input } from "../ui/input";
import { SiteAvatar } from "./SiteAvatar";
import {
  attachSiteLink,
  cancelLogin,
  cancelWebLogin,
  detectSite,
  isCommandError,
  isPreview,
  listSites,
  loadSiteLink,
  loginSite,
  setCurrentSite,
  submitLoginCode,
  submitLoginPassword,
  webLogin,
  type LoginOutcome,
  type SiteInfo,
  type SiteLink,
  type SiteView,
} from "../../lib/api";
import { settingsKey } from "../../lib/currentSite";
import { errorText } from "../../lib/errors";

type Step =
  | { kind: "url" }
  | { kind: "link" }
  | { kind: "login"; info: SiteInfo }
  | { kind: "code"; info: SiteInfo; flowId: string; stage: "login" | "token" }
  | { kind: "password"; info: SiteInfo; flowId: string }
  /** 登录成功了，但没能记下站长配置（例如配置里的站点地址和登录的站点不一致）。 */
  | { kind: "attachFailed"; site: SiteView; message: string };

/** 输入的是专属链接（或者直接是 TransitHub 的配置地址），而不是站点网址。 */
const looksLikeLink = (value: string) => value.trim().startsWith("transitlink://") || value.includes("/api/client/v1/sites/");

/**
 * 接入站点：输入网址 → 识别 → 账号密码登录 → （需要时）两步验证码。
 * 开了人机验证的站点改为在登录窗口里登录（站点自己的登录页），new-api 之后再输一次密码创建长期令牌。
 * 用专属链接接入时，先显示站长的品牌，登录成功后记下站长配置。
 */
export function AddSiteDialog({ open, initialUrl = "", initialAccount = "", initialLink, onClose, onAdded }: {
  open: boolean;
  initialUrl?: string;
  initialAccount?: string;
  /** 专属链接：打开时直接读取。 */
  initialLink?: string;
  onClose: () => void;
  /** `viaLink`：是用专属链接接入的（接下来自动打开一键配置）。 */
  onAdded: (site: SiteView, viaLink: boolean) => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [step, setStep] = useState<Step>({ kind: "url" });
  const [url, setUrl] = useState(initialUrl);
  const [account, setAccount] = useState(initialAccount);
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [waitingWeb, setWaitingWeb] = useState(false);
  const [link, setLink] = useState<SiteLink | null>(null);
  const sites = useQuery({ queryKey: ["sites"], queryFn: listSites, enabled: open });
  // 专属链接指向的站点已经接入过：可以直接换上站长的品牌，不用重新登录。
  const existing = link ? (sites.data ?? []).filter((site) => site.baseUrl === link.config.site.baseUrl) : [];
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setStep({ kind: "url" });
    setUrl(initialUrl);
    setAccount(initialAccount);
    setPassword("");
    setCode("");
    setError(null);
    setWaitingWeb(false);
    setLink(null);
    if (initialLink) {
      setUrl(initialLink);
      void run(async () => openLink(initialLink));
    }
    // run 和 openLink 每次渲染都是新的函数，只在打开时执行一次。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, initialUrl, initialAccount, initialLink]);

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (caught) {
      setError(errorText(t, caught));
    } finally {
      setBusy(false);
    }
  };

  const close = () => {
    if (step.kind === "code" || step.kind === "password") void cancelLogin(step.flowId);
    if (waitingWeb) void cancelWebLogin();
    onClose();
  };

  /** 登录（或下一步）的结果：完成，或者转到验证码、密码步骤。 */
  const handle = async (info: SiteInfo, outcome: LoginOutcome) => {
    if (outcome.status === "done") await finish(outcome.site);
    else if (outcome.status === "needPassword") setStep({ kind: "password", info, flowId: outcome.flowId });
    else setStep({ kind: "code", info, flowId: outcome.flowId, stage: outcome.stage });
  };

  const onWebLogin = async (info: SiteInfo) => {
    setWaitingWeb(true);
    setError(null);
    try {
      await handle(info, await webLogin(info.baseUrl));
    } catch (caught) {
      // 用户自己关掉了登录窗口：回到按钮，不算出错。
      if (!(isCommandError(caught) && caught.code === "loginCancelled")) setError(errorText(t, caught));
    } finally {
      setWaitingWeb(false);
    }
  };

  const openLink = async (value: string) => {
    setLink(await loadSiteLink(value));
    setStep({ kind: "link" });
  };

  /** 新接入的站点（或刚换上站长品牌的站点）成为当前站点；只是重新登录的不动。工具不跟着切。 */
  const makeCurrent = async (site: SiteView) => {
    try {
      queryClient.setQueryData(settingsKey, await setCurrentSite(site.id));
    } catch {
      // 记不下来也不影响接入，界面会自动挑一个站点。
    }
  };

  const finish = async (site: SiteView) => {
    setPassword("");
    if (!(sites.data ?? []).some((item) => item.id === site.id)) await makeCurrent(site);
    if (!link) {
      onAdded(site, false);
      return;
    }
    // 记下站长配置。失败时站点照样接入，但要告诉用户没能换上品牌。
    try {
      onAdded(await attachSiteLink(site.id, link.configUrl), true);
    } catch (caught) {
      setStep({ kind: "attachFailed", site, message: errorText(t, caught) });
    }
  };

  const onUseExisting = () => {
    if (!link) return;
    void run(async () => {
      let updated: SiteView | null = null;
      for (const site of existing) updated = await attachSiteLink(site.id, link.configUrl);
      if (updated) {
        await makeCurrent(updated);
        onAdded(updated, false);
      }
    });
  };

  const onDetect = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (looksLikeLink(url)) {
      void run(async () => openLink(url));
      return;
    }
    setLink(null);
    void run(async () => setStep({ kind: "login", info: await detectSite(url) }));
  };

  const onLinkContinue = () => {
    if (!link) return;
    void run(async () => setStep({ kind: "login", info: await detectSite(link.config.site.baseUrl) }));
  };

  const onLogin = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (step.kind !== "login") return;
    void run(async () => handle(step.info, await loginSite(step.info.baseUrl, account, password)));
  };

  const onCode = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (step.kind !== "code") return;
    void run(async () => {
      const outcome = await submitLoginCode(step.flowId, code);
      setCode("");
      await handle(step.info, outcome);
    });
  };

  const onPassword = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (step.kind !== "password") return;
    void run(async () => handle(step.info, await submitLoginPassword(step.flowId, password)));
  };

  return (
    <Dialog open={open} title={t("sites.add.title")} onClose={close} closeLabel={t("common.close")}>
      {step.kind === "url" && (
        <form onSubmit={onDetect} className="space-y-4">
          <Field label={t("sites.add.url")} htmlFor="site-url" hint={t("sites.add.urlOrLinkHint")}>
            <Input id="site-url" value={url} onChange={(event) => setUrl(event.target.value)} placeholder="https://api.example.com" autoFocus required />
          </Field>
          {error && <ErrorLine text={error} />}
          <Button type="submit" className="w-full" disabled={busy || !url.trim()}>
            {busy && <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />}
            {t("sites.add.detect")}
          </Button>
        </form>
      )}

      {step.kind === "link" && link && (
        <div className="space-y-4">
          <BrandHeader link={link} />
          {link.config.brand.welcome && <p className="text-sm text-muted-foreground">{link.config.brand.welcome}</p>}
          {existing.length > 0 ? (
            <p className="rounded-lg bg-brand/10 p-3 text-sm">
              {t("sites.add.linkExisting", { accounts: existing.map((site) => site.account).join(t("common.separator")) })}
            </p>
          ) : (
            <p className="text-xs text-muted-foreground">{t("sites.add.linkHint")}</p>
          )}
          {error && <ErrorLine text={error} />}
          <div className="flex gap-2">
            <Button variant="secondary" onClick={() => setStep({ kind: "url" })} disabled={busy}>
              {t("common.back")}
            </Button>
            {existing.length > 0 ? (
              <>
                <Button variant="secondary" className="flex-1" onClick={onLinkContinue} disabled={busy}>
                  {t("sites.add.otherAccount")}
                </Button>
                <Button className="flex-1" onClick={onUseExisting} disabled={busy}>
                  {busy && <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />}
                  {t("sites.add.useExisting")}
                </Button>
              </>
            ) : (
              <Button className="flex-1" onClick={onLinkContinue} disabled={busy}>
                {busy && <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />}
                {t("sites.add.linkContinue")}
              </Button>
            )}
          </div>
        </div>
      )}

      {step.kind === "attachFailed" && (
        <div className="space-y-4">
          <Notice text={t("sites.add.attachFailed", { error: step.message })} />
          <Button className="w-full" onClick={() => onAdded(step.site, true)}>
            {t("sites.add.continue")}
          </Button>
        </div>
      )}

      {step.kind !== "url" && step.kind !== "link" && step.kind !== "attachFailed" && (link ? <BrandHeader link={link} /> : <SiteHeader info={step.info} />)}

      {step.kind === "login" && step.info.captchaRequired && (
        <div className="mt-4 space-y-4">
          <p className="flex gap-2 rounded-lg bg-muted p-3 text-sm" role="status">
            <ShieldCheck className="mt-0.5 h-4 w-4 shrink-0 text-brand" aria-hidden />
            {t("sites.add.captchaRequired")}
          </p>
          {isPreview && <p className="text-xs text-muted-foreground">{t("sites.add.previewWebHint")}</p>}
          {error && <ErrorLine text={error} />}
          {waitingWeb ? (
            <div className="flex items-center gap-3 rounded-lg border border-border p-3 text-sm">
              <LoaderCircle className="h-4 w-4 shrink-0 animate-spin text-muted-foreground" aria-hidden />
              <span className="flex-1">{t("sites.add.waitingWeb")}</span>
              <Button variant="ghost" size="sm" onClick={() => void cancelWebLogin()}>
                {t("common.cancel")}
              </Button>
            </div>
          ) : (
            <div className="flex gap-2">
              <Button variant="secondary" onClick={() => setStep({ kind: "url" })}>
                {t("common.back")}
              </Button>
              <Button className="flex-1" onClick={() => void onWebLogin(step.info)}>
                <ExternalLink className="h-4 w-4" aria-hidden />
                {t("sites.add.openLoginWindow")}
              </Button>
            </div>
          )}
        </div>
      )}

      {step.kind === "login" && !step.info.captchaRequired && (
        <form onSubmit={onLogin} className="mt-4 space-y-4">
          {step.info.passwordLoginDisabled && <Notice text={t("sites.add.passwordLoginDisabled")} />}
          <Field label={step.info.emailOnly ? t("sites.add.email") : t("sites.add.account")} htmlFor="site-account">
            <Input id="site-account" value={account} onChange={(event) => setAccount(event.target.value)} type={step.info.emailOnly ? "email" : "text"} autoComplete="username" autoFocus required />
          </Field>
          <Field label={t("sites.add.password")} htmlFor="site-password" hint={t("sites.add.passwordHint")}>
            <Input id="site-password" value={password} onChange={(event) => setPassword(event.target.value)} type="password" autoComplete="current-password" required />
          </Field>
          {isPreview && <p className="text-xs text-muted-foreground">{t("sites.add.previewHint")}</p>}
          {error && <ErrorLine text={error} />}
          <div className="flex gap-2">
            <Button variant="secondary" onClick={() => setStep({ kind: "url" })} disabled={busy}>
              {t("common.back")}
            </Button>
            <Button type="submit" className="flex-1" disabled={busy || step.info.passwordLoginDisabled}>
              {busy && <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />}
              {t("sites.add.login")}
            </Button>
          </div>
        </form>
      )}

      {step.kind === "password" && (
        <form onSubmit={onPassword} className="mt-4 space-y-4">
          <p className="text-sm text-muted-foreground">{t("sites.add.tokenPasswordHint")}</p>
          <Field label={t("sites.add.password")} htmlFor="site-token-password">
            <Input id="site-token-password" value={password} onChange={(event) => setPassword(event.target.value)} type="password" autoComplete="current-password" autoFocus required />
          </Field>
          {error && <ErrorLine text={error} />}
          <Button type="submit" className="w-full" disabled={busy || !password}>
            {busy && <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />}
            {t("sites.add.verify")}
          </Button>
        </form>
      )}

      {step.kind === "code" && (
        <form onSubmit={onCode} className="mt-4 space-y-4">
          <p className="text-sm text-muted-foreground">{step.stage === "token" ? t("sites.add.tokenCodeHint") : t("sites.add.codeHint")}</p>
          <Field label={t("sites.add.code")} htmlFor="site-code">
            <Input
              id="site-code"
              value={code}
              onChange={(event) => setCode(event.target.value.replace(/\D/g, "").slice(0, 6))}
              inputMode="numeric"
              autoComplete="one-time-code"
              className="text-center font-mono text-lg tracking-[0.4em]"
              autoFocus
              required
            />
          </Field>
          {error && <ErrorLine text={error} />}
          <Button type="submit" className="w-full" disabled={busy || code.length !== 6}>
            {busy && <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />}
            {t("sites.add.verify")}
          </Button>
        </form>
      )}
    </Dialog>
  );
}

/** 用专属链接接入时显示站长的品牌：logo（没有时用主题色和名称首字）、名称和站点地址。 */
function BrandHeader({ link }: { link: SiteLink }) {
  const { brand, site } = link.config;
  const name = brand.name || site.name;
  return (
    <div className="flex items-center gap-3 rounded-lg bg-muted p-3">
      {brand.logoUrl ? (
        <img src={brand.logoUrl} alt="" className="h-9 w-9 shrink-0 rounded-lg object-cover" />
      ) : (
        <span
          className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg text-sm font-semibold text-white"
          style={{ backgroundColor: brand.color || "var(--brand)" }}
          aria-hidden
        >
          {name.slice(0, 1)}
        </span>
      )}
      <div className="min-w-0 flex-1">
        <p className="truncate font-medium">{name}</p>
        <p className="truncate text-xs text-muted-foreground">{site.baseUrl}</p>
      </div>
      <Badge tone="brand">{site.platform === "newapi" ? "new-api" : "sub2api"}</Badge>
    </div>
  );
}

function SiteHeader({ info }: { info: SiteInfo }) {
  return (
    <div className="flex items-center gap-3 rounded-lg bg-muted p-3">
      <SiteAvatar name={info.name} logoUrl={info.logoUrl} />
      <div className="min-w-0 flex-1">
        <p className="truncate font-medium">{info.name}</p>
        <p className="truncate text-xs text-muted-foreground">{info.baseUrl}</p>
      </div>
      <Badge tone="brand">{info.platform === "newapi" ? "new-api" : "sub2api"}</Badge>
    </div>
  );
}

function Notice({ text }: { text: string }) {
  return (
    <p className="flex gap-2 rounded-lg bg-danger/10 p-3 text-sm text-danger" role="status">
      <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
      {text}
    </p>
  );
}

function ErrorLine({ text }: { text: string }) {
  return (
    <p className="text-sm text-danger" role="alert">
      {text}
    </p>
  );
}
