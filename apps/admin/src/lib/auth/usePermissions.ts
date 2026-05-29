import { useMemo } from "react";
import type { Permission } from "./AuthContext";
import { useAuth } from "./useAuth";

/**
 * Thin selector over `useAuth().permissions`. Not an independent query — it
 * shares render cadence with the rest of the auth-derived UI so action gates
 * don't flicker between the auth state and the permission state.
 */
export function usePermissions() {
  const { permissions } = useAuth();
  return useMemo(
    () => ({
      permissions,
      has: (p: Permission) => permissions.includes(p),
      hasAny: (perms: readonly Permission[]) =>
        perms.some((p) => permissions.includes(p)),
      hasAll: (perms: readonly Permission[]) =>
        perms.every((p) => permissions.includes(p)),
    }),
    [permissions],
  );
}
