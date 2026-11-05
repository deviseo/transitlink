import { useCallback, useEffect, useState, type KeyboardEvent, type SubmitEvent } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft, CircleCheck, Headset, LoaderCircle, Lock, Plus, Send, UserRound } from "lucide-react";
import { AttachmentThumb, ImageStrip, ImageViewer, useImageAttachments, type ViewedImage } from "../components/tickets/images";
import { NewTicketDialog } from "../components/tickets/NewTicketDialog";
import { CharCount, StatusBadge, TicketConsent, TicketProblem } from "../components/tickets/parts";
import { Button } from "../components/ui/button";
import { Card } from "../components/ui/card";
import { Field, Textarea } from "../components/ui/input";
import { closeTicket, isCommandError, replyTicket, ticketDetail, ticketSession, type TicketDetail, type TicketList, type TicketMessage, type TicketSession } from "../lib/api";
import { cn } from "../lib/cn";
import { errorText } from "../lib/errors";
import { messageTime, syncList, ticketDetailOptions, ticketKeys, toUploads, useTicketSites } from "../lib/tickets";

const MAX_BODY = 5000;

/** 一个工单：来回的消息、图片，回复和关闭。打开后客服的回复就算看过了。 */
export function TicketDetailPage() {
  const { siteId = "", ticketId = "" } = useParams();
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { sites, loaded } = useTicketSites();
  const site = sites.find((item) => item.id === siteId);
  const allowed = site?.ticketsAllowed === true;
  const detail = useQuery({
    queryKey: ticketKeys.detail(siteId, ticketId),
    queryFn: () => ticketDetail(siteId, ticketId),
    enabled: allowed,
    ...ticketDetailOptions,
  });
  const session = useQuery({ queryKey: ticketKeys.session(siteId), queryFn: () => ticketSession(siteId), enabled: allowed, staleTime: 10 * 60 * 1000 });
  const [viewing, setViewing] = useState<ViewedImage | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [creating, setCreating] = useState(false);
  const closeViewer = useCallback(() => setViewing(null), []);

  // 详情是最新的：列表里的状态、时间和未读数跟着更新（左边导航的数字也跟着变）。
  useEffect(() => {
    const ticket = detail.data;
    if (ticket) queryClient.setQueryData<TicketList>(ticketKeys.list(siteId), (list) => syncList(list, ticket));
  }, [detail.data, queryClient, siteId]);

  const update = (ticket: TicketDetail) => queryClient.setQueryData(ticketKeys.detail(siteId, ticketId), ticket);
  const close = useMutation({
    mutationFn: () => closeTicket(siteId, ticketId),
    onSuccess: (ticket) => {
      setConfirming(false);
      update(ticket);
    },
  });

  const back = (
    <Link to="/tickets" className="inline-flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand">
      <ArrowLeft className="h-4 w-4" aria-hidden />
      {t("tickets.back")}
    </Link>
  );

  if (loaded && !site) {
    return (
      <div className="space-y-6">
        {back}
        <p className="rounded-lg bg-muted p-3 text-sm text-muted-foreground">{t("tickets.noSite")}</p>
      </div>
    );
  }

  if (site && !allowed) {
    return (
      <div className="space-y-6">
        {back}
        <TicketConsent site={site} />
      </div>
    );
  }

  const ticket = detail.data;
  return (
    <div className="space-y-6">
      {back}
      {(!loaded || detail.isLoading) && <LoaderCircle className="h-5 w-5 animate-spin text-muted-foreground" aria-hidden />}
      {detail.isError && !ticket && <TicketProblem error={detail.error} onRetry={() => void detail.refetch()} />}

      {ticket && site && (
        <>
          <header className="flex flex-wrap items-start justify-between gap-4">
            <div className="min-w-0 flex-1 space-y-1.5">
              <div className="flex flex-wrap items-center gap-2">
                <h1 className="min-w-0 break-words text-xl font-semibold">{ticket.title}</h1>
                <StatusBadge status={ticket.status} />
              </div>
              <p className="text-xs text-muted-foreground">
                {[
                  ticket.category,
                  t("tickets.createdAt", { time: messageTime(ticket.createdAt, i18n.language) }),
                  ticket.contact && t("tickets.contactShown", { contact: ticket.contact }),
                ]
                  .filter(Boolean)
                  .join(" · ")}
              </p>
            </div>
            {ticket.status !== "closed" &&
              (confirming ? (
                <div className="flex flex-wrap items-center gap-2 text-sm">
                  <span className="text-muted-foreground">{t("tickets.closeConfirm")}</span>
                  <Button variant="ghost" size="sm" onClick={() => setConfirming(false)} disabled={close.isPending}>
                    {t("common.cancel")}
                  </Button>
                  <Button size="sm" onClick={() => close.mutate()} disabled={close.isPending}>
                    {close.isPending && <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />}
                    {t("tickets.closeTicket")}
                  </Button>
                </div>
              ) : (
                <Button variant="secondary" size="sm" onClick={() => setConfirming(true)}>
                  <CircleCheck className="h-4 w-4" aria-hidden />
                  {t("tickets.closeTicket")}
                </Button>
              ))}
          </header>
          {close.isError && <TicketProblem error={close.error} />}

          <Card>
            <ol className="space-y-5">
              {ticket.messages.map((message) => (
                <MessageItem key={message.id} message={message} siteId={siteId} onOpenImage={setViewing} />
              ))}
            </ol>
          </Card>

          {ticket.status === "closed" ? (
            <div className="flex flex-wrap items-center gap-3 rounded-lg border border-border bg-muted/60 p-4 text-sm">
              <Lock className="h-4 w-4 shrink-0 text-muted-foreground" aria-hidden />
              <span className="min-w-0 flex-1 text-muted-foreground">{t("tickets.closedNotice")}</span>
              <Button size="sm" onClick={() => setCreating(true)}>
                <Plus className="h-4 w-4" aria-hidden />
                {t("tickets.new")}
              </Button>
            </div>
          ) : (
            <ReplyBox
              siteId={siteId}
              ticketId={ticketId}
              limits={session.data?.limits}
              onSent={update}
              onClosedMeanwhile={() => void queryClient.invalidateQueries({ queryKey: ticketKeys.detail(siteId, ticketId) })}
            />
          )}
        </>
      )}

      <ImageViewer image={viewing} onClose={closeViewer} />
      {creating && site && (
        <NewTicketDialog
          site={site}
          onClose={() => setCreating(false)}
          onCreated={(created) => {
            setCreating(false);
            queryClient.setQueryData(ticketKeys.detail(siteId, created.id), created);
            queryClient.setQueryData<TicketList>(ticketKeys.list(siteId), (list) => syncList(list, created));
            void navigate(`/tickets/${siteId}/${created.id}`);
          }}
        />
      )}
    </div>
  );
}

/** 一条消息：自己的在右边，客服的在左边。 */
function MessageItem({ message, siteId, onOpenImage }: { message: TicketMessage; siteId: string; onOpenImage: (image: ViewedImage) => void }) {
  const { t, i18n } = useTranslation();
  const mine = message.authorType === "customer";
  const author = mine ? t("tickets.me") : message.authorName ? `${t("tickets.support")} · ${message.authorName}` : t("tickets.support");
  return (
    <li className={cn("flex gap-3", mine && "flex-row-reverse")}>
      <span
        className={cn("flex h-8 w-8 shrink-0 items-center justify-center rounded-full", mine ? "bg-brand/15 text-brand" : "bg-muted text-foreground")}
        aria-hidden
      >
        {mine ? <UserRound className="h-4 w-4" /> : <Headset className="h-4 w-4" />}
      </span>
      <div className={cn("flex min-w-0 max-w-[80%] flex-col", mine ? "items-end" : "items-start")}>
        <p className="text-xs text-muted-foreground">
          <span className="font-medium text-foreground">{author}</span>
          {" · "}
          <time dateTime={message.createdAt}>{messageTime(message.createdAt, i18n.language)}</time>
        </p>
        {message.body && (
          <div className={cn("mt-1.5 whitespace-pre-wrap break-words rounded-xl px-3.5 py-2.5 text-sm leading-6", mine ? "rounded-tr-sm bg-brand/10" : "rounded-tl-sm bg-muted")}>
            {message.body}
          </div>
        )}
        {message.attachments.length > 0 && (
          <div className={cn("mt-2 flex flex-wrap gap-2", mine && "justify-end")}>
            {message.attachments.map((attachment) => (
              <AttachmentThumb key={attachment.id} siteId={siteId} attachment={attachment} onOpen={onOpenImage} />
            ))}
          </div>
        )}
      </div>
    </li>
  );
}

/** 回复框：文字和图片（可以只发图片），⌘/Ctrl + Enter 发送。 */
function ReplyBox({
  siteId,
  ticketId,
  limits,
  onSent,
  onClosedMeanwhile,
}: {
  siteId: string;
  ticketId: string;
  limits: TicketSession["limits"] | undefined;
  onSent: (ticket: TicketDetail) => void;
  /** 客服在这期间关掉了工单：重新读一下，界面换成「已关闭」。 */
  onClosedMeanwhile: () => void;
}) {
  const { t } = useTranslation();
  const [body, setBody] = useState("");
  const attachments = useImageAttachments(limits);
  const send = useMutation({
    mutationFn: () => replyTicket(siteId, ticketId, body, toUploads(attachments.images)),
    onSuccess: (ticket) => {
      setBody("");
      attachments.clear();
      onSent(ticket);
    },
    onError: (error) => {
      if (isCommandError(error) && error.code === "ticketsClosed") onClosedMeanwhile();
    },
  });
  const ready = (body.trim() !== "" || attachments.images.length > 0) && !send.isPending && !attachments.busy;

  const onSubmit = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (ready) send.mutate();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      event.currentTarget.form?.requestSubmit();
    }
  };

  return (
    <Card>
      <form onSubmit={onSubmit} className="space-y-3">
        <Field label={t("tickets.reply")} htmlFor="ticket-reply" aside={<CharCount value={body} max={MAX_BODY} />}>
          <Textarea
            id="ticket-reply"
            value={body}
            onChange={(event) => setBody(event.target.value)}
            onPaste={attachments.onPaste}
            onKeyDown={onKeyDown}
            maxLength={MAX_BODY}
            rows={4}
            placeholder={t("tickets.replyPlaceholder")}
          />
        </Field>
        <ImageStrip attachments={attachments} disabled={send.isPending} />
        {send.isError && (
          <p className="text-sm text-danger" role="alert">
            {errorText(t, send.error)}
          </p>
        )}
        <div className="flex justify-end">
          <Button type="submit" disabled={!ready}>
            {send.isPending ? <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden /> : <Send className="h-4 w-4" aria-hidden />}
            {t("tickets.send")}
          </Button>
        </div>
      </form>
    </Card>
  );
}
