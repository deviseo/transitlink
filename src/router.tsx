import { createHashRouter } from "react-router";
import { AppLayout } from "./components/layout/AppLayout";
import { HomePage } from "./pages/HomePage";
import { SettingsPage } from "./pages/SettingsPage";
import { SitesPage } from "./pages/SitesPage";
import { TicketDetailPage } from "./pages/TicketDetailPage";
import { TicketsPage } from "./pages/TicketsPage";
import { ToolsPage } from "./pages/ToolsPage";

// 桌面应用用 hash 路由：页面从本地文件加载，不依赖服务器的路径回退。
export const router = createHashRouter([
  {
    path: "/",
    element: <AppLayout />,
    children: [
      { index: true, element: <HomePage /> },
      { path: "sites", element: <SitesPage /> },
      { path: "tools", element: <ToolsPage /> },
      { path: "tickets", element: <TicketsPage /> },
      { path: "tickets/:siteId/:ticketId", element: <TicketDetailPage /> },
      { path: "settings", element: <SettingsPage /> },
    ],
  },
]);
