import { useCallback, useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { AddSiteDialog } from "../sites/AddSiteDialog";
import { SetupDialog } from "../tools/SetupDialog";
import { refreshSiteLinks, takePendingLink } from "../../lib/api";
import { inDesktop } from "../../lib/tauri";

/**
 * 专属链接：应用被链接打开（或运行中又点了链接）时，打开接入窗口并显示站长的品牌；
 * 接入后接着打开一键配置。启动时顺便更新已接入站点的站长配置。
 */
export function LinkEvents() {
  const queryClient = useQueryClient();
  const [link, setLink] = useState<string | null>(null);
  const [setupSite, setSetupSite] = useState<string | null>(null);

  const takeLink = useCallback(async () => {
    const pending = await takePendingLink().catch(() => null);
    if (pending) setLink(pending);
  }, []);

  useEffect(() => {
    if (!inDesktop) return;
    void takeLink();
    // 站长改了品牌、推荐或公告：启动时和之后每半小时检查一次（桌面端只重新读取超过 10 分钟的配置）。
    const refresh = () =>
      void refreshSiteLinks()
        .then((changed) => {
          if (changed) void queryClient.invalidateQueries({ queryKey: ["sites"] });
        })
        .catch(() => undefined);
    refresh();
    const timer = window.setInterval(refresh, 30 * 60 * 1000);
    const stop = listen("site-link", () => void takeLink());
    return () => {
      window.clearInterval(timer);
      void stop.then((unlisten) => unlisten());
    };
  }, [takeLink, queryClient]);

  return (
    <>
      <AddSiteDialog
        open={link !== null}
        initialLink={link ?? undefined}
        onClose={() => setLink(null)}
        onAdded={(site, viaLink) => {
          setLink(null);
          void queryClient.invalidateQueries({ queryKey: ["sites"] });
          // 新登录的站点接着一键配置；已经接入过、只是换上品牌的不打扰。
          if (viaLink) setSetupSite(site.id);
        }}
      />
      <SetupDialog open={setupSite !== null} initialSiteId={setupSite ?? undefined} onClose={() => setSetupSite(null)} />
    </>
  );
}
