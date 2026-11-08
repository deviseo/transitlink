import { useState, type SubmitEvent } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Copy, KeyRound, LoaderCircle, Plus } from "lucide-react";
import { Badge, type Tone } from "../ui/badge";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { copySiteKey, createSiteKey, siteGroups, siteKeys, type KeyStatus, type KeySummary } from "../../lib/api";

const statusTone: Record<KeyStatus, Tone> = { active: "success", disabled: "neutral", expired: "warning", exhausted: "warning" };
import { errorText } from "../../lib/errors";

/** 某个站点的 Key 列表和新建 Key（密钥页）。列表里的 Key 都是打码的，复制时由桌面端取完整 Key 写进剪贴板。 */
export function KeysPanel({ siteId }: { siteId: string }) {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const keys = useQuery({ queryKey: ["site-keys", siteId], queryFn: () => siteKeys(siteId) });
  const money = new Intl.NumberFormat(i18n.language, { style: "currency", currency: "USD", maximumFractionDigits: 2 });
  const limits = (key: KeySummary) =>
    [
      key.remainingUsd != null ? t("keys.remaining", { amount: money.format(key.remainingUsd) }) : null,
      key.expiresAt != null ? t("keys.expires", { date: new Date(key.expiresAt * 1000).toLocaleDateString(i18n.language) }) : null,
    ].filter(Boolean);
  const groups = useQuery({ queryKey: ["site-groups", siteId], queryFn: () => siteGroups(siteId) });
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("TransitLink");
  const [group, setGroup] = useState("");
  const [created, setCreated] = useState<KeySummary | null>(null);
  const [copyError, setCopyError] = useState<unknown>(null);

  const create = useMutation({
    mutationFn: () => createSiteKey(siteId, name, group || null),
    onSuccess: (key) => {
      setCreated(key);
      setCreating(false);
      void queryClient.invalidateQueries({ queryKey: ["site-keys", siteId] });
    },
  });

  const onCreate = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    create.mutate();
  };

  return (
    <div>
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-medium">{t("keys.list")}</h2>
        {!creating && (
          <Button variant="secondary" size="sm" onClick={() => setCreating(true)}>
            <Plus className="h-4 w-4" aria-hidden />
            {t("keys.create")}
          </Button>
        )}
      </div>

      {creating && (
        <form onSubmit={onCreate} className="mt-3 flex flex-wrap items-end gap-2 rounded-lg bg-muted p-3">
          <label className="min-w-40 flex-1 space-y-1 text-xs text-muted-foreground">
            {t("keys.name")}
            <Input value={name} onChange={(event) => setName(event.target.value)} required />
          </label>
          <label className="min-w-40 flex-1 space-y-1 text-xs text-muted-foreground">
            {t("keys.group")}
            <select
              value={group}
              onChange={(event) => setGroup(event.target.value)}
              className="h-9 w-full rounded-lg border border-border bg-card px-2 text-sm text-foreground outline-none focus:border-brand focus:ring-1 focus:ring-brand"
            >
              <option value="">{t("keys.defaultGroup")}</option>
              {(groups.data ?? []).map((item) => (
                <option key={item.id} value={item.id}>
                  {item.name}
                  {item.ratio != null ? ` ×${item.ratio}` : ""}
                  {item.description ? ` · ${item.description}` : ""}
                </option>
              ))}
            </select>
          </label>
          <div className="flex gap-2">
            <Button variant="ghost" size="sm" className="h-9" onClick={() => setCreating(false)}>
              {t("common.cancel")}
            </Button>
            <Button type="submit" size="sm" className="h-9" disabled={create.isPending || !name.trim()}>
              {create.isPending && <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />}
              {t("keys.createSubmit")}
            </Button>
          </div>
          {create.isError && <p className="w-full text-sm text-danger">{errorText(t, create.error)}</p>}
        </form>
      )}

      {created && (
        <div className="mt-3 flex items-center gap-3 rounded-lg border border-success/30 bg-success/10 p-3 text-sm">
          <div className="min-w-0 flex-1">
            <p className="font-medium text-success">{t("keys.created")}</p>
            <p className="truncate font-mono text-xs text-muted-foreground">
              {created.name} · {created.key}
            </p>
          </div>
          <CopyKeyButton siteId={siteId} keyId={created.id} onError={setCopyError} labelled />
        </div>
      )}

      {keys.isLoading && <LoaderCircle className="mt-3 h-4 w-4 animate-spin text-muted-foreground" aria-hidden />}
      {keys.isError && <p className="mt-3 text-sm text-danger">{errorText(t, keys.error)}</p>}
      {keys.data && keys.data.length === 0 && <p className="mt-3 text-sm text-muted-foreground">{t("keys.empty")}</p>}
      {keys.data && keys.data.length > 0 && (
        <ul className="mt-3 divide-y divide-border">
          {keys.data.map((key) => (
            <li key={key.id} className="flex items-center gap-3 py-2 text-sm">
              <KeyRound className="h-4 w-4 shrink-0 text-muted-foreground" aria-hidden />
              <div className="min-w-0 flex-1">
                <p className="truncate">{key.name || t("keys.unnamed")}</p>
                <p className="truncate text-xs text-muted-foreground">
                  <span className="font-mono">{key.key}</span>
                  {limits(key).map((limit) => (
                    <span key={limit}> · {limit}</span>
                  ))}
                </p>
              </div>
              {key.group && <Badge>{key.group}</Badge>}
              <Badge tone={statusTone[key.status]}>{t(`keys.status.${key.status}`)}</Badge>
              <CopyKeyButton siteId={siteId} keyId={key.id} onError={setCopyError} />
            </li>
          ))}
        </ul>
      )}
      {copyError != null && <p className="mt-2 text-sm text-danger">{errorText(t, copyError)}</p>}
    </div>
  );
}

/** 复制完整 Key。`labelled` 时显示文字，列表里只显示图标。 */
function CopyKeyButton({
  siteId,
  keyId,
  onError,
  labelled = false,
}: {
  siteId: string;
  keyId: string;
  onError: (error: unknown) => void;
  labelled?: boolean;
}) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const copy = useMutation({
    mutationFn: () => copySiteKey(siteId, keyId),
    onMutate: () => onError(null),
    onSuccess: () => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    },
    onError,
  });
  const label = copied ? t("common.copied") : t("keys.copy");
  return (
    <Button
      variant={labelled ? "secondary" : "ghost"}
      size="sm"
      onClick={() => copy.mutate()}
      disabled={copy.isPending}
      aria-label={label}
      title={label}
    >
      {copy.isPending ? (
        <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />
      ) : copied ? (
        <Check className="h-4 w-4 text-success" aria-hidden />
      ) : (
        <Copy className="h-4 w-4" aria-hidden />
      )}
      {labelled && label}
    </Button>
  );
}
