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
    void refreshSiteLinks()
      .then((changed) => {
        if (changed) void queryClient.invalidateQueries({ queryKey: ["sites"] });
      })
      .catch(() => undefined);
    const stop = listen("site-link", () => void takeLink());
    return () => {
      void stop.then((unlisten) => unlisten());
    };
  }, [takeLink, queryClient]);

  return (
    <>
      <AddSiteDialog
        open={link !== null}
        initialLink={link ?? undefined}
        onClose={() => setLink(null)}
        onAdded={(site) => {
          setLink(null);
          void queryClient.invalidateQueries({ queryKey: ["sites"] });
          setSetupSite(site.id);
        }}
      />
      <SetupDialog open={setupSite !== null} initialSiteId={setupSite ?? undefined} onClose={() => setSetupSite(null)} />
    </>
  );
}
