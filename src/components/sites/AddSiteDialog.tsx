import { useEffect, useState, type SubmitEvent } from "react";
import { useTranslation } from "react-i18next";
import { ExternalLink, LoaderCircle, ShieldAlert, ShieldCheck } from "lucide-react";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Dialog } from "../ui/dialog";
import { Field, Input } from "../ui/input";
import { SiteAvatar } from "./SiteAvatar";
import {
  cancelLogin,
  cancelWebLogin,
  detectSite,
  isCommandError,
  isPreview,
  loginSite,
  submitLoginCode,
  submitLoginPassword,
  webLogin,
  type LoginOutcome,
  type SiteInfo,
  type SiteView,
} from "../../lib/api";
import { errorText } from "../../lib/errors";

type Step =
  | { kind: "url" }
  | { kind: "login"; info: SiteInfo }
  | { kind: "code"; info: SiteInfo; flowId: string; stage: "login" | "token" }
  | { kind: "password"; info: SiteInfo; flowId: string };

/**
 * 接入站点：输入网址 → 识别 → 账号密码登录 → （需要时）两步验证码。
 * 开了人机验证的站点改为在登录窗口里登录（站点自己的登录页），new-api 之后再输一次密码创建长期令牌。
 */
export function AddSiteDialog({ open, initialUrl = "", initialAccount = "", onClose, onAdded }: {
  open: boolean;
  initialUrl?: string;
  initialAccount?: string;
  onClose: () => void;
  onAdded: (site: SiteView) => void;
}) {
  const { t } = useTranslation();
  const [step, setStep] = useState<Step>({ kind: "url" });
  const [url, setUrl] = useState(initialUrl);
  const [account, setAccount] = useState(initialAccount);
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [waitingWeb, setWaitingWeb] = useState(false);
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
  }, [open, initialUrl, initialAccount]);

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
  const handle = (info: SiteInfo, outcome: LoginOutcome) => {
    if (outcome.status === "done") finish(outcome.site);
    else if (outcome.status === "needPassword") setStep({ kind: "password", info, flowId: outcome.flowId });
    else setStep({ kind: "code", info, flowId: outcome.flowId, stage: outcome.stage });
  };

  const onWebLogin = async (info: SiteInfo) => {
    setWaitingWeb(true);
    setError(null);
    try {
      handle(info, await webLogin(info.baseUrl));
    } catch (caught) {
      // 用户自己关掉了登录窗口：回到按钮，不算出错。
      if (!(isCommandError(caught) && caught.code === "loginCancelled")) setError(errorText(t, caught));
    } finally {
      setWaitingWeb(false);
    }
  };

  const finish = (site: SiteView) => {
    setPassword("");
    onAdded(site);
  };

  const onDetect = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    void run(async () => setStep({ kind: "login", info: await detectSite(url) }));
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
      handle(step.info, outcome);
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
          <Field label={t("sites.add.url")} htmlFor="site-url" hint={t("sites.add.urlHint")}>
            <Input id="site-url" value={url} onChange={(event) => setUrl(event.target.value)} placeholder="https://api.example.com" autoFocus required />
          </Field>
          {error && <ErrorLine text={error} />}
          <Button type="submit" className="w-full" disabled={busy || !url.trim()}>
            {busy && <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />}
            {t("sites.add.detect")}
          </Button>
        </form>
      )}

      {step.kind !== "url" && <SiteHeader info={step.info} />}

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
