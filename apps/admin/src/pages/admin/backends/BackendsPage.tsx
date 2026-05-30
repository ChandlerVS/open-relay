import { useMemo, useState } from "react";
import { MoreHorizontal, Plug, Plus } from "lucide-react";
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
import {
  useBackendsList,
  type BackendInstanceDto,
} from "../../../lib/backends/useBackends";
import {
  BackendInUseError,
  useDeleteBackend,
} from "../../../lib/backends/useBackendMutations";
import { BackendFormDialog } from "./BackendFormDialog";

export function BackendsPage() {
  const { has } = usePermissions();
  const { data, isLoading, isError, error, refetch } = useBackendsList();

  const [createOpen, setCreateOpen] = useState(false);
  const [editing, setEditing] = useState<BackendInstanceDto | null>(null);
  const [deleting, setDeleting] = useState<BackendInstanceDto | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [deleteInUse, setDeleteInUse] = useState<BackendInUseError | null>(null);
  const deleteMutation = useDeleteBackend();

  const canWrite = has("backends:write");
  const canDelete = has("backends:delete");

  const summary = useMemo(() => {
    if (!data) return null;
    if (data.total === 0) return "No backends configured yet.";
    return `${data.total} configured`;
  }, [data]);

  return (
    <div className="space-y-6 max-w-5xl">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Backends</h1>
          <p className="text-sm text-muted-foreground">
            Configured delivery destinations. Attach them to forms to fan out
            submissions.
          </p>
        </div>
        <RequirePermission perm="backends:write">
          <Button onClick={() => setCreateOpen(true)}>
            <Plus className="h-4 w-4" />
            New backend
          </Button>
        </RequirePermission>
      </div>

      {isError && (
        <Alert variant="destructive">
          <AlertTitle>Couldn't load backends</AlertTitle>
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
              <TableHead>Kind</TableHead>
              <TableHead>Updated</TableHead>
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
                    <Skeleton className="h-4 w-40" />
                  </TableCell>
                  <TableCell>
                    <Skeleton className="h-4 w-28" />
                  </TableCell>
                  <TableCell>
                    <Skeleton className="h-4 w-24" />
                  </TableCell>
                  <TableCell />
                </TableRow>
              ))
            ) : data && data.items.length > 0 ? (
              data.items.map((b) => (
                <TableRow key={b.id}>
                  <TableCell className="font-medium flex items-center gap-2">
                    <Plug className="h-4 w-4 text-muted-foreground" />
                    {b.name}
                  </TableCell>
                  <TableCell>
                    <code className="text-xs rounded bg-muted px-1.5 py-0.5">
                      {b.kind}
                    </code>
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {new Date(b.updated_at).toLocaleDateString()}
                  </TableCell>
                  <TableCell className="text-right pr-2">
                    {(canWrite || canDelete) && (
                      <RowMenu
                        canWrite={canWrite}
                        canDelete={canDelete}
                        onEdit={() => setEditing(b)}
                        onDelete={() => {
                          setDeleteError(null);
                          setDeleteInUse(null);
                          setDeleting(b);
                        }}
                      />
                    )}
                  </TableCell>
                </TableRow>
              ))
            ) : (
              <TableRow>
                <TableCell
                  colSpan={4}
                  className="text-center py-10 text-sm text-muted-foreground"
                >
                  No backends configured. Click "New backend" to add one.
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
        {summary && (
          <div className="border-t border-border px-3 py-2 text-sm text-muted-foreground">
            {summary}
          </div>
        )}
      </div>

      <BackendFormDialog open={createOpen} onOpenChange={setCreateOpen} />
      <BackendFormDialog
        open={editing !== null}
        onOpenChange={(o) => !o && setEditing(null)}
        existing={editing}
      />
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(o) => {
          if (!o) {
            setDeleting(null);
            setDeleteError(null);
            setDeleteInUse(null);
          }
        }}
        title="Delete backend?"
        description={
          <span>
            This permanently removes{" "}
            <span className="font-medium text-foreground">
              {deleting?.name}
            </span>
            . Submissions queued for delivery to it will fail permanently.
            {deleteInUse && (
              <span className="mt-2 block text-destructive">
                Still attached to:{" "}
                {deleteInUse.forms.map((f, i) => (
                  <span key={f.id}>
                    {i > 0 && ", "}
                    <span className="font-medium">{f.name}</span>
                  </span>
                ))}
                . Unbind it from those forms first.
              </span>
            )}
            {deleteError && !deleteInUse && (
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
                setDeleteInUse(null);
              },
              onError: (err) => {
                if (err instanceof BackendInUseError) {
                  setDeleteInUse(err);
                } else {
                  setDeleteError(err.message);
                }
              },
            },
          );
        }}
      />
    </div>
  );
}

interface RowMenuProps {
  canWrite: boolean;
  canDelete: boolean;
  onEdit: () => void;
  onDelete: () => void;
}

function RowMenu({ canWrite, canDelete, onEdit, onDelete }: RowMenuProps) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="sm" aria-label="Row actions">
          <MoreHorizontal className="h-4 w-4" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        {canWrite && <DropdownMenuItem onSelect={onEdit}>Edit</DropdownMenuItem>}
        {canWrite && canDelete && <DropdownMenuSeparator />}
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
