import { Navigate, type RouteObject } from "react-router-dom";

import { HistoryPage } from "./HistoryPage";
import { OverviewPage } from "./OverviewPage";
import { MoreRedirect } from "./phone/SettingsHome";
import { SessionTerminalRoute } from "./phone/PhoneTerminal";
import { ProjectPage } from "./ProjectPage";
import { RecipesPage } from "./RecipesPage";
import { SessionDetailPage } from "./SessionDetailPage";
import { SettingsPage } from "./SettingsPage";
import { SplitViewPage } from "./SplitViewPage";

/**
 * The pages under /app, relative to it. One table for the router (sidebar
 * and phone shells render it through <Outlet/>) and for the glass shell's
 * page frames, which render it for their own location.
 */
export const APP_PAGES: RouteObject[] = [
  { index: true, element: <OverviewPage /> },
  { path: "overview", element: <OverviewPage /> },
  { path: "sessions/:id", element: <SessionDetailPage /> },
  { path: "sessions/:id/terminal", element: <SessionTerminalRoute /> },
  { path: "split", element: <SplitViewPage /> },
  { path: "projects/:id", element: <ProjectPage /> },
  { path: "history", element: <HistoryPage /> },
  { path: "recipes", element: <RecipesPage /> },
  { path: "recipes/:id", element: <RecipesPage /> },
  { path: "more", element: <MoreRedirect /> },
  { path: "settings", element: <SettingsPage /> },
  { path: "settings/:tab", element: <SettingsPage /> },
  { path: "*", element: <Navigate to="/app/overview" replace /> },
];
