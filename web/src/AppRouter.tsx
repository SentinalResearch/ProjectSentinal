import { BrowserRouter, Navigate, Route, Routes } from "react-router-dom";

import { SetupPage } from "./routes/auth/SetupPage";
import { LoginPage } from "./routes/auth/LoginPage";
import { StartupPage } from "./routes/auth/StartupPage";
import { AppShell } from "./routes/app/AppShell";
import { APP_PAGES } from "./routes/app/pages";
import { PublicAuthGuard, RequireAuth } from "./routes/guards";

export function AppRouter() {
  return (
    <BrowserRouter>
      <Routes>
        {/* Startup: reaches the server, then hands off to sign-in, first run or the app. */}
        <Route path="/" element={<StartupPage />} />

        <Route element={<PublicAuthGuard />}>
          <Route path="/login" element={<LoginPage />} />
          <Route path="/setup" element={<SetupPage />} />
        </Route>

        <Route element={<RequireAuth />}>
          {/* "/*": the glass shell renders its page frames as descendant routes. */}
          <Route path="/app/*" element={<AppShell />}>
            {APP_PAGES.map((page) =>
              page.index ? (
                <Route key="index" index element={page.element} />
              ) : (
                <Route key={page.path} path={page.path} element={page.element} />
              ),
            )}
          </Route>
        </Route>

        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </BrowserRouter>
  );
}
