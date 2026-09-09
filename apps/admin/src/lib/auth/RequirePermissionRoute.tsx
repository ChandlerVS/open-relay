import { Outlet } from "react-router-dom";
import { Skeleton } from "@open-relay/ui";
import type { Permission } from "./AuthContext";
import { NoAccess } from "./NoAccess";
import { useAuth } from "./useAuth";
import { usePermissions } from "./usePermissions";

export interface RequirePermissionRouteProps {
  perm: Permission;
  /** Phrase for the denial copy, e.g. "view users". */
  action?: string;
}

/**
 * Route-level counterpart to `RequirePermission`. Mount it as a pathless
 * wrapper route around the pages a permission covers; it renders `<Outlet />`
 * when the session holds it and `NoAccess` when it doesn't.
 *
 * The `loading` branch is load-bearing: `permissions` is `[]` until `/auth/me`
 * resolves, so checking it while the session is still hydrating would flash
 * the denial panel on every hard refresh.
 */
export function RequirePermissionRoute({
  perm,
  action,
}: RequirePermissionRouteProps) {
  const { status } = useAuth();
  const { has } = usePermissions();

  if (status === "loading") {
    return (
      <div className="space-y-3 max-w-xl">
        <Skeleton className="h-8 w-48" />
        <Skeleton className="h-4 w-72" />
      </div>
    );
  }
  if (!has(perm)) return <NoAccess perm={perm} action={action} />;
  return <Outlet />;
}
