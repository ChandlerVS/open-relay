import { Shield } from "lucide-react";
import { Skeleton } from "@open-relay/ui";
import { PermissionNotice } from "../../lib/auth/PermissionNotice";
import { useAuth } from "../../lib/auth/useAuth";
import { usePermissions } from "../../lib/auth/usePermissions";
import { useRoleSelectList } from "../../lib/roles/useRoles";

export interface RoleMultiSelectProps {
  value: number[];
  onChange: (next: number[]) => void;
  disabled?: boolean;
}

/**
 * Compact checkbox-list role picker. Renders flat (no grouping) since roles
 * are admin-defined and typically fit on a single screen. Backed by
 * `/roles/select-list` which returns lightweight summaries.
 *
 * Two permission facts shape it. `roles:assign` (checked by the caller) is
 * what gates the whole control; `roles:read` is separately needed to *list*
 * the roles, and the two are independent grants — so the list can be
 * unreadable to someone who may legitimately assign.
 */
export function RoleMultiSelect({ value, onChange, disabled }: RoleMultiSelectProps) {
  const { roles: myRoles } = useAuth();
  const canReadRoles = usePermissions().has("roles:read");
  const { data, isLoading, isError, error } = useRoleSelectList({
    enabled: canReadRoles,
  });

  // Mirrors the server's escalation guard in `rbac::service::assign_roles_to_user`:
  // only a superadmin may grant the system role. Derivable here because
  // `/auth/me` returns `is_system` on the caller's own roles.
  const isSuperadmin = myRoles.some((r) => r.is_system);

  if (!canReadRoles) {
    return (
      <PermissionNotice
        perm="roles:read"
        action="see or change role assignments"
        current={
          value.length > 0
            ? `${value.length} role${value.length === 1 ? " is" : "s are"} assigned and will be kept.`
            : undefined
        }
      />
    );
  }
  if (isLoading) return <Skeleton className="h-20 w-full" />;
  if (isError) {
    return (
      <p className="text-sm text-destructive">
        {(error as Error | undefined)?.message ?? "Failed to load roles."}
      </p>
    );
  }
  if (!data || data.length === 0) {
    return (
      <p className="text-xs text-muted-foreground">
        No roles defined yet. Create one in the Roles page.
      </p>
    );
  }
  return (
    <div className="rounded border border-border divide-y divide-border max-h-48 overflow-y-auto">
      {data.map((r) => {
        const checked = value.includes(r.id);
        // Never lock someone out of *unchecking* a role already assigned —
        // the server bounds newly-added roles only.
        const reserved = r.is_system && !isSuperadmin && !checked;
        const locked = disabled || reserved;
        return (
          <label
            key={r.id}
            title={
              reserved ? "Only a superadmin may assign this role." : undefined
            }
            className={
              locked
                ? "flex items-center gap-2 px-3 py-2 text-sm select-none opacity-60 cursor-not-allowed"
                : "flex items-center gap-2 px-3 py-2 text-sm cursor-pointer select-none hover:bg-accent/40"
            }
          >
            <input
              type="checkbox"
              checked={checked}
              disabled={locked}
              onChange={(e) => {
                const next = e.target.checked
                  ? [...value, r.id]
                  : value.filter((v) => v !== r.id);
                onChange(next);
              }}
              className="h-4 w-4 rounded border-border accent-primary"
            />
            <span className="flex-1">{r.name}</span>
            {r.is_system && (
              <span
                title="System-managed role"
                className="inline-flex items-center gap-1 rounded bg-muted px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wider text-muted-foreground"
              >
                <Shield className="h-3 w-3" />
                system
              </span>
            )}
          </label>
        );
      })}
    </div>
  );
}
