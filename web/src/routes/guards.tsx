import { Navigate, Outlet, useLocation } from "react-router-dom";

import { useAuth } from "../context/AuthContext";

/** `/login` and `/setup` — steer signed-in users into the app, and first-run users into setup. */
export function PublicAuthGuard() {
  const { status, loading } = useAuth();
  const location = useLocation();

  if (loading) return null;
  if (status === "signedIn") return <Navigate to="/app/overview" replace />;
  if (status === "setup" && location.pathname !== "/setup") return <Navigate to="/setup" replace />;
  if (status !== "setup" && location.pathname === "/setup") return <Navigate to="/login" replace />;
  return <Outlet />;
}

/** `/app/*` — everything behind sign-in. */
export function RequireAuth() {
  const { status, loading } = useAuth();

  if (loading) return null;
  if (status === "setup") return <Navigate to="/setup" replace />;
  if (status !== "signedIn") return <Navigate to="/login" replace />;
  return <Outlet />;
}
