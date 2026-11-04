import { useState, type SubmitEvent } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery } from "@tanstack/react-query";
import { LoaderCircle, Send } from "lucide-react";
import { Button } from "../ui/button";
import { Dialog } from "../ui/dialog";
import { Field, Input, Textarea } from "../ui/input";
import { ImageStrip, useImageAttachments } from "./images";
import { CharCount, TicketProblem } from "./parts";
import { createTicket, ticketSession, type SiteView, type TicketDetail } from "../../lib/api";
import { errorText } from "../../lib/errors";
import { ticketKeys, toUploads } from "../../lib/tickets";

const MAX_TITLE = 100;
const MAX_BODY = 5000;
const MAX_CONTACT = 100;

/** 新建工单：问题分类、标题、问题描述、图片和联系方式（选填）。父组件需要时才渲染，每次打开都是空白的。 */
export function NewTicketDialog({ site, onClose, onCreated }: { site: SiteView; onClose: () => void; onCreated: (ticket: TicketDetail) => void }) {
  const { t } = useTranslation();
  const session = useQuery({ queryKey: ticketKeys.session(site.id), queryFn: () => ticketSession(site.id), staleTime: 10 * 60 * 1000 });
  const [category, setCategory] = useState("");
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [contact, setContact] = useState("");
  const attachments = useImageAttachments(session.data?.limits);
  const chosenCategory = category || session.data?.categories[0] || "";
  const create = useMutation({
    mutationFn: () => createTicket(site.id, { title, category: chosenCategory, body, contact }, toUploads(attachments.images)),
    onSuccess: onCreated,
  });
  const ready = title.trim() !== "" && body.trim() !== "" && !create.isPending;

  // 提交中不能关掉，免得以为没提交上。
  const close = () => {
    if (!create.isPending) onClose();
  };

  const onSubmit = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (ready) create.mutate();
  };

  return (
    <Dialog open title={t("tickets.new")} onClose={close} closeLabel={t("common.close")} wide>
      {session.isLoading && <LoaderCircle className="h-5 w-5 animate-spin text-muted-foreground" aria-hidden />}
      {session.isError && <TicketProblem error={session.error} onRetry={() => void session.refetch()} />}
      {session.data && (
        <form onSubmit={onSubmit} className="space-y-4">
          <div className="grid gap-4 sm:grid-cols-[11rem_1fr]">
            <Field label={t("tickets.category")} htmlFor="ticket-category">
              <select
                id="ticket-category"
                value={chosenCategory}
                onChange={(event) => setCategory(event.target.value)}
                className="h-9 w-full min-w-0 rounded-lg border border-border bg-card px-2 text-sm text-foreground outline-none focus:border-brand focus:ring-1 focus:ring-brand"
              >
                {session.data.categories.map((item) => (
                  <option key={item} value={item}>
                    {item}
                  </option>
                ))}
              </select>
            </Field>
            <Field label={t("tickets.titleLabel")} htmlFor="ticket-title">
              <Input id="ticket-title" value={title} onChange={(event) => setTitle(event.target.value)} maxLength={MAX_TITLE} placeholder={t("tickets.titlePlaceholder")} autoFocus required />
            </Field>
          </div>
          <Field label={t("tickets.body")} htmlFor="ticket-body" aside={<CharCount value={body} max={MAX_BODY} />}>
            <Textarea
              id="ticket-body"
              value={body}
              onChange={(event) => setBody(event.target.value)}
              onPaste={attachments.onPaste}
              maxLength={MAX_BODY}
              rows={6}
              placeholder={t("tickets.bodyPlaceholder")}
              required
            />
          </Field>
          {attachments.enabled && (
            <div className="space-y-1.5">
              <p className="text-sm font-medium">{t("tickets.images")}</p>
              <ImageStrip attachments={attachments} disabled={create.isPending} />
            </div>
          )}
          <Field label={t("tickets.contact")} htmlFor="ticket-contact" hint={t("tickets.contactHint")}>
            <Input id="ticket-contact" value={contact} onChange={(event) => setContact(event.target.value)} maxLength={MAX_CONTACT} />
          </Field>
          {create.isError && (
            <p className="text-sm text-danger" role="alert">
              {errorText(t, create.error)}
            </p>
          )}
          <div className="flex justify-end gap-2">
            <Button variant="secondary" onClick={close} disabled={create.isPending}>
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={!ready}>
              {create.isPending ? <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden /> : <Send className="h-4 w-4" aria-hidden />}
              {t("tickets.submit")}
            </Button>
          </div>
        </form>
      )}
    </Dialog>
  );
}
