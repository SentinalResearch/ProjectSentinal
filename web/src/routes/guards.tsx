import { Navigate, Outlet, useLocation } from "react-router-dom";

import { useAuth } from "../context/AuthContext";

/** `/login` and `/setup` — steer signed-in users into the app, and first-run users into setup. */
export function PublicAuthGuard() {
  const { status, loading } = useAuth();
  const location = useLocation();

  if (loading) return null;
  const requested = safeReturnPath(
    typeof location.state === "object" && location.state !== null && "returnTo" in location.state
      ? String(location.state.returnTo)
      : null,
  );
  if (status === "signedIn") return <Navigate to={requested ?? "/app/overview"} replace />;
  if (status === "setup" && location.pathname !== "/setup") return <Navigate to="/setup" replace />;
  if (status !== "setup" && location.pathname === "/setup") return <Navigate to="/login" replace />;
  return <Outlet />;
}

/** `/app/*` — everything behind sign-in. */
export function RequireAuth() {
  const { status, loading } = useAuth();
  const location = useLocation();

  if (loading) return null;
  if (status === "setup") return <Navigate to="/setup" replace />;
  if (status !== "signedIn") {
    const returnTo = safeReturnPath(`${location.pathname}${location.search}${location.hash}`);
    return <Navigate to="/login" replace state={{ returnTo }} />;
  }
  return <Outlet />;
}

/** Accept a bounded same-origin application route for post-login navigation. */
export function safeReturnPath(value: string | null): string | null {
  if (value === null || value.length === 0 || value.length > 2_048) return null;
  if (!value.startsWith("/app") || value.startsWith("//")) return null;
  if (value.includes("\\") || value.includes("#") || /%(?:2f|5c|2e)/i.test(value)) return null;
  if ([...value].some((character) => character.charCodeAt(0) < 32 || character.charCodeAt(0) === 127)) return null;
  try {
    const parsed = new URL(value, "https://sentinal.invalid");
    if (parsed.origin !== "https://sentinal.invalid") return null;
    if (parsed.pathname !== "/app" && !parsed.pathname.startsWith("/app/")) return null;
    return `${parsed.pathname}${parsed.search}`;
  } catch {
    return null;
  }
}
