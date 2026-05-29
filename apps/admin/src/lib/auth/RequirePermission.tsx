import type { ReactNode } from "react";
import type { Permission } from "./AuthContext";
import { usePermissions } from "./usePermissions";

interface CommonProps {
  children: ReactNode;
  fallback?: ReactNode;
}

type PermissionProps =
  | { perm: Permission; any?: never; all?: never }
  | { perm?: never; any: readonly Permission[]; all?: never }
  | { perm?: never; any?: never; all: readonly Permission[] };

type Props = CommonProps & PermissionProps;

/**
 * Render children only when the current session holds the required
 * permission(s). Use `perm` for a single check, `any` for an OR-set, or
 * `all` for an AND-set. Falls back to `fallback` (default: null).
 *
 *   <RequirePermission perm="users:write"><Button>New user</Button></RequirePermission>
 *   <RequirePermission any={["users:write", "users:delete"]}>...</RequirePermission>
 */
export function RequirePermission({
  children,
  fallback = null,
  perm,
  any,
  all,
}: Props) {
  const { has, hasAny, hasAll } = usePermissions();
  const allowed =
    perm !== undefined
      ? has(perm)
      : any !== undefined
        ? hasAny(any)
        : hasAll(all!);
  return <>{allowed ? children : fallback}</>;
}
