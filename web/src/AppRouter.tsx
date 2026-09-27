import { BrowserRouter, Route, Routes } from "react-router-dom";

import { SetupPage } from "./routes/auth/SetupPage";
import { LoginPage } from "./routes/auth/LoginPage";
import { AppShell } from "./routes/app/AppShell";
import { OverviewPage } from "./routes/app/OverviewPage";
import { SessionDetailPage } from "./routes/app/SessionDetailPage";
import { SplitViewPage } from "./routes/app/SplitViewPage";
import { ProjectPage } from "./routes/app/ProjectPage";
import { HistoryPage } from "./routes/app/HistoryPage";
import { RecipesPage } from "./routes/app/RecipesPage";
import { SettingsPage } from "./routes/app/SettingsPage";
import { LandingPage } from "./routes/landing/LandingPage";
import { PublicAuthGuard, RequireAuth } from "./routes/guards";

export function AppRouter() {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="/" element={<LandingPage />} />

        <Route element={<PublicAuthGuard />}>
          <Route path="/login" element={<LoginPage />} />
          <Route path="/setup" element={<SetupPage />} />
        </Route>

        <Route element={<RequireAuth />}>
          <Route path="/app" element={<AppShell />}>
            <Route index element={<OverviewPage />} />
            <Route path="overview" element={<OverviewPage />} />
            <Route path="sessions/:id" element={<SessionDetailPage />} />
            <Route path="split" element={<SplitViewPage />} />
            <Route path="projects/:id" element={<ProjectPage />} />
            <Route path="history" element={<HistoryPage />} />
            <Route path="recipes" element={<RecipesPage />} />
            <Route path="recipes/:id" element={<RecipesPage />} />
            <Route path="settings" element={<SettingsPage />} />
            <Route path="settings/:tab" element={<SettingsPage />} />
          </Route>
        </Route>

        <Route path="*" element={<LandingPage />} />
      </Routes>
    </BrowserRouter>
  );
}
