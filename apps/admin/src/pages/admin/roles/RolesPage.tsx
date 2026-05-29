import { useState } from "react";
import { MoreHorizontal, Plus, Shield } from "lucide-react";
import {
  Alert,
  AlertDescription,
  AlertTitle,
  Button,
  ConfirmDialog,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  Skeleton,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@open-relay/ui";
import { RequirePermission } from "../../../lib/auth/RequirePermission";
import { usePermissions } from "../../../lib/auth/usePermissions";
import { useRolesList, type RoleDto } from "../../../lib/roles/useRoles";
import { useDeleteRole } from "../../../lib/roles/useRoleMutations";
import { RoleFormDialog } from "./RoleFormDialog";

export function RolesPage() {
  const { data, isLoading, isError, error, refetch } = useRolesList();
  const { has } = usePermissions();
  const [createOpen, setCreateOpen] = useState(false);
  const [editing, setEditing] = useState<RoleDto | null>(null);
  const [deleting, setDeleting] = useState<RoleDto | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const deleteMutation = useDeleteRole();

  const canWrite = has("roles:write");
  const canDelete = has("roles:delete");

  return (
    <div className="space-y-6 max-w-5xl">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Roles</h1>
          <p className="text-sm text-muted-foreground">
            Bundle permissions into named roles, then assign them to users from
            the user form. Permissions themselves live in the codebase; new
            permissions appear here automatically on deploy.
          </p>
        </div>
        <RequirePermission perm="roles:write">
          <Button onClick={() => setCreateOpen(true)}>
            <Plus className="h-4 w-4" />
            New role
          </Button>
        </RequirePermission>
      </div>

      {isError && (
        <Alert variant="destructive">
          <AlertTitle>Couldn't load roles</AlertTitle>
          <AlertDescription>
            {(error as Error | undefined)?.message ?? "Unknown error."}{" "}
            <button
              type="button"
              className="underline font-medium"
              onClick={() => refetch()}
            >
              Try again
            </button>
          </AlertDescription>
        </Alert>
      )}

      <div className="border border-border rounded-lg bg-background">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Name</TableHead>
              <TableHead>Permissions</TableHead>
              <TableHead className="w-10 text-right pr-3">
                <span className="sr-only">Actions</span>
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {isLoading ? (
              Array.from({ length: 3 }).map((_, i) => (
                <TableRow key={`s-${i}`}>
                  <TableCell>
                    <Skeleton className="h-4 w-32" />
                  </TableCell>
                  <TableCell>
                    <Skeleton className="h-4 w-64" />
                  </TableCell>
                  <TableCell />
                </TableRow>
              ))
            ) : data && data.length > 0 ? (
              data.map((r) => (
                <TableRow key={r.id}>
                  <TableCell className="font-medium align-top">
                    <div className="flex items-center gap-2">
                      <span>{r.name}</span>
                      {r.is_system && (
                        <span
                          title="System-managed role"
                          className="inline-flex items-center gap-1 rounded bg-muted px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wider text-muted-foreground"
                        >
                          <Shield className="h-3 w-3" />
                          system
                        </span>
                      )}
                    </div>
                    {r.description && (
                      <p className="mt-1 text-xs text-muted-foreground font-normal">
                        {r.description}
                      </p>
                    )}
                  </TableCell>
                  <TableCell className="align-top">
                    {r.permissions.length === 0 ? (
                      <span className="text-muted-foreground text-xs">
                        No permissions granted
                      </span>
                    ) : (
                      <div className="flex flex-wrap gap-1">
                        {r.permissions.map((p) => (
                          <span
                            key={p}
                            className="rounded bg-secondary px-1.5 py-0.5 text-[11px] font-mono text-secondary-foreground"
                          >
                            {p}
                          </span>
                        ))}
                      </div>
                    )}
                  </TableCell>
                  <TableCell className="text-right pr-2 align-top">
                    {(canWrite || canDelete) && !r.is_system && (
                      <RowMenu
                        canEdit={canWrite}
                        canDelete={canDelete}
                        onEdit={() => setEditing(r)}
                        onDelete={() => {
                          setDeleteError(null);
                          setDeleting(r);
                        }}
                      />
                    )}
                  </TableCell>
                </TableRow>
              ))
            ) : (
              <TableRow>
                <TableCell colSpan={3} className="text-center py-10 text-sm text-muted-foreground">
                  No roles defined yet.
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>

      <RoleFormDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
      />
      <RoleFormDialog
        open={editing !== null}
        onOpenChange={(o) => !o && setEditing(null)}
        existingRole={editing}
      />
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(o) => {
          if (!o) {
            setDeleting(null);
            setDeleteError(null);
          }
        }}
        title="Delete role?"
        description={
          <span>
            This permanently removes{" "}
            <span className="font-medium text-foreground">{deleting?.name}</span>{" "}
            and unassigns it from every user.
            {deleteError && (
              <span className="mt-2 block text-destructive">{deleteError}</span>
            )}
          </span>
        }
        confirmLabel="Delete"
        pending={deleteMutation.isPending}
        onConfirm={() => {
          if (!deleting) return;
          deleteMutation.mutate(
            { id: deleting.id },
            {
              onSuccess: () => {
                setDeleting(null);
                setDeleteError(null);
              },
              onError: (err) => setDeleteError(err.message),
            },
          );
        }}
      />
    </div>
  );
}

interface RowMenuProps {
  canEdit: boolean;
  canDelete: boolean;
  onEdit: () => void;
  onDelete: () => void;
}

function RowMenu({ canEdit, canDelete, onEdit, onDelete }: RowMenuProps) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="sm" aria-label="Row actions">
          <MoreHorizontal className="h-4 w-4" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        {canEdit && <DropdownMenuItem onSelect={onEdit}>Edit</DropdownMenuItem>}
        {canEdit && canDelete && <DropdownMenuSeparator />}
        {canDelete && (
          <DropdownMenuItem
            onSelect={onDelete}
            className="text-destructive focus:text-destructive"
          >
            Delete
          </DropdownMenuItem>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
