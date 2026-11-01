import { BrowserRouter, Route, Routes } from "react-router-dom";

import { SetupPage } from "./routes/auth/SetupPage";
import { LoginPage } from "./routes/auth/LoginPage";
import { AppShell } from "./routes/app/AppShell";
import { APP_PAGES } from "./routes/app/pages";
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

        <Route path="*" element={<LandingPage />} />
      </Routes>
    </BrowserRouter>
  );
}
