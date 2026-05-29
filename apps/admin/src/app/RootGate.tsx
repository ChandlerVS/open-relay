import { Navigate, Outlet, useLocation } from "react-router-dom";
import { Skeleton } from "@open-relay/ui";
import { useAuth } from "../lib/auth/useAuth";
import { useSetupStatus } from "../lib/setup/useSetupStatus";

/**
 * Single source of truth for which route the user is allowed on, based on
 * (a) whether the system is initialized and (b) auth status. Pages stay
 * dumb; all redirect logic lives here.
 */
export function RootGate() {
  const location = useLocation();
  const setup = useSetupStatus();
  const auth = useAuth();

  if (setup.isPending || auth.status === "loading") {
    return <BootSkeleton />;
  }

  // If the backend is unreachable, bail out cleanly rather than redirect-looping.
  if (setup.isError || !setup.data) {
    return (
      <main className="min-h-screen grid place-items-center bg-background text-foreground p-8">
        <div className="text-center space-y-2">
          <h1 className="text-lg font-semibold">Backend unreachable</h1>
          <p className="text-sm text-muted-foreground">
            Couldn't fetch /setup/status. Is the API server running?
          </p>
        </div>
      </main>
    );
  }

  const path = location.pathname;
  const isAuth = auth.status === "authenticated";
  // An authenticated session proves the system is initialized — even if the
  // cached /setup/status response still says otherwise (it's polled lazily).
  const initialized = isAuth || setup.data.initialized;

  // The OAuth callback page handles its own auth handoff (it reads a token
  // out of the URL fragment, calls signInWithToken, then redirects). It must
  // load even when the user is anonymous.
  if (path === "/oauth/callback") {
    return <Outlet />;
  }

  if (isAuth && (path === "/login" || path === "/setup")) {
    return <Navigate to="/" replace />;
  }
  if (!initialized && path !== "/setup") {
    return <Navigate to="/setup" replace />;
  }
  if (initialized && path === "/setup") {
    return <Navigate to="/login" replace />;
  }
  if (!isAuth && path !== "/login" && path !== "/setup") {
    return <Navigate to="/login" replace state={{ from: location }} />;
  }

  return <Outlet />;
}

function BootSkeleton() {
  return (
    <main className="min-h-screen bg-background text-foreground p-8">
      <div className="mx-auto max-w-md space-y-3">
        <Skeleton className="h-8 w-40" />
        <Skeleton className="h-4 w-72" />
        <Skeleton className="h-4 w-56" />
      </div>
    </main>
  );
}
