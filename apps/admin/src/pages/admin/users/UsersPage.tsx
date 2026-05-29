import { useMemo, useState } from "react";
import { ChevronLeft, ChevronRight, MoreHorizontal, Plus, Shield } from "lucide-react";
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
import { useAuth } from "../../../lib/auth/useAuth";
import { RequirePermission } from "../../../lib/auth/RequirePermission";
import { usePermissions } from "../../../lib/auth/usePermissions";
import { useUsersList, type UserDto } from "../../../lib/users/useUsers";
import { useDeleteUser } from "../../../lib/users/useUserMutations";
import { UserFormDialog } from "./UserFormDialog";
import { ChangePasswordDialog } from "./ChangePasswordDialog";

const PAGE_SIZE = 25;

export function UsersPage() {
  const { user: currentUser } = useAuth();
  const { has } = usePermissions();
  const [offset, setOffset] = useState(0);
  const { data, isLoading, isError, error, refetch } = useUsersList({
    limit: PAGE_SIZE,
    offset,
  });

  const [createOpen, setCreateOpen] = useState(false);
  const [editing, setEditing] = useState<UserDto | null>(null);
  const [pwUser, setPwUser] = useState<UserDto | null>(null);
  const [deleting, setDeleting] = useState<UserDto | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const deleteMutation = useDeleteUser();

  const canWrite = has("users:write");
  const canDelete = has("users:delete");

  const rangeText = useMemo(() => {
    if (!data) return null;
    if (data.total === 0) return "No users yet.";
    const start = data.offset + 1;
    const end = Math.min(data.offset + data.items.length, data.total);
    return `Showing ${start}–${end} of ${data.total}`;
  }, [data]);

  const canPrev = offset > 0;
  const canNext = data ? offset + data.items.length < data.total : false;

  return (
    <div className="space-y-6 max-w-5xl">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Users</h1>
          <p className="text-sm text-muted-foreground">
            Manage admin accounts and their role assignments.
          </p>
        </div>
        <RequirePermission perm="users:write">
          <Button onClick={() => setCreateOpen(true)}>
            <Plus className="h-4 w-4" />
            New user
          </Button>
        </RequirePermission>
      </div>

      {isError && (
        <Alert variant="destructive">
          <AlertTitle>Couldn't load users</AlertTitle>
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
              <TableHead>Display name</TableHead>
              <TableHead>Email</TableHead>
              <TableHead>Roles</TableHead>
              <TableHead className="w-10 text-right pr-3">
                <span className="sr-only">Actions</span>
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {isLoading ? (
              Array.from({ length: 4 }).map((_, i) => (
                <TableRow key={`s-${i}`}>
                  <TableCell>
                    <Skeleton className="h-4 w-32" />
                  </TableCell>
                  <TableCell>
                    <Skeleton className="h-4 w-48" />
                  </TableCell>
                  <TableCell>
                    <Skeleton className="h-4 w-24" />
                  </TableCell>
                  <TableCell />
                </TableRow>
              ))
            ) : data && data.items.length > 0 ? (
              data.items.map((u) => (
                <TableRow key={u.id}>
                  <TableCell className="font-medium">
                    {u.display_name?.trim() ? (
                      u.display_name
                    ) : (
                      <span className="text-muted-foreground">—</span>
                    )}
                  </TableCell>
                  <TableCell>{u.email}</TableCell>
                  <TableCell>
                    {(u.roles ?? []).length === 0 ? (
                      <span className="text-xs text-muted-foreground">—</span>
                    ) : (
                      <div className="flex flex-wrap gap-1">
                        {(u.roles ?? []).map((r) => (
                          <span
                            key={r.id}
                            className={
                              r.is_system
                                ? "inline-flex items-center gap-1 rounded bg-primary/15 px-1.5 py-0.5 text-[11px] font-medium text-primary"
                                : "rounded bg-secondary px-1.5 py-0.5 text-[11px] text-secondary-foreground"
                            }
                          >
                            {r.is_system && <Shield className="h-3 w-3" />}
                            {r.name}
                          </span>
                        ))}
                      </div>
                    )}
                  </TableCell>
                  <TableCell className="text-right pr-2">
                    {(canWrite || canDelete) && (
                      <RowMenu
                        user={u}
                        isCurrent={currentUser?.id === u.id}
                        canWrite={canWrite}
                        canDelete={canDelete}
                        onEdit={() => setEditing(u)}
                        onChangePassword={() => setPwUser(u)}
                        onDelete={() => {
                          setDeleteError(null);
                          setDeleting(u);
                        }}
                      />
                    )}
                  </TableCell>
                </TableRow>
              ))
            ) : (
              <TableRow>
                <TableCell colSpan={4} className="text-center py-10 text-sm text-muted-foreground">
                  No users to show.
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
        <div className="flex items-center justify-between border-t border-border px-3 py-2 text-sm text-muted-foreground">
          <div>{rangeText}</div>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={!canPrev || isLoading}
              onClick={() => setOffset((o) => Math.max(0, o - PAGE_SIZE))}
            >
              <ChevronLeft className="h-4 w-4" />
              Prev
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={!canNext || isLoading}
              onClick={() => setOffset((o) => o + PAGE_SIZE)}
            >
              Next
              <ChevronRight className="h-4 w-4" />
            </Button>
          </div>
        </div>
      </div>

      <UserFormDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
      />
      <UserFormDialog
        open={editing !== null}
        onOpenChange={(o) => !o && setEditing(null)}
        existingUser={editing}
      />
      <ChangePasswordDialog
        open={pwUser !== null}
        onOpenChange={(o) => !o && setPwUser(null)}
        user={pwUser}
      />
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(o) => {
          if (!o) {
            setDeleting(null);
            setDeleteError(null);
          }
        }}
        title="Delete user?"
        description={
          <span>
            This permanently removes{" "}
            <span className="font-medium text-foreground">
              {deleting?.display_name?.trim() || deleting?.email}
            </span>
            . They will no longer be able to sign in.
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
  user: UserDto;
  isCurrent: boolean;
  canWrite: boolean;
  canDelete: boolean;
  onEdit: () => void;
  onChangePassword: () => void;
  onDelete: () => void;
}

function RowMenu({
  isCurrent,
  canWrite,
  canDelete,
  onEdit,
  onChangePassword,
  onDelete,
}: RowMenuProps) {
  const showDeleteSection = canDelete && !isCurrent;
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="sm" aria-label="Row actions">
          <MoreHorizontal className="h-4 w-4" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        {canWrite && <DropdownMenuItem onSelect={onEdit}>Edit</DropdownMenuItem>}
        {canWrite && (
          <DropdownMenuItem onSelect={onChangePassword}>
            Change password
          </DropdownMenuItem>
        )}
        {canWrite && showDeleteSection && <DropdownMenuSeparator />}
        {showDeleteSection && (
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
