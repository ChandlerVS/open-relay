import { Shield } from "lucide-react";
import { Skeleton } from "@open-relay/ui";
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
 */
export function RoleMultiSelect({ value, onChange, disabled }: RoleMultiSelectProps) {
  const { data, isLoading, isError, error } = useRoleSelectList();
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
        return (
          <label
            key={r.id}
            className="flex items-center gap-2 px-3 py-2 text-sm cursor-pointer select-none hover:bg-accent/40"
          >
            <input
              type="checkbox"
              checked={checked}
              disabled={disabled}
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
