import { useMemo, useState } from "react";
import { Contact, MoreHorizontal, Plus } from "lucide-react";
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
import { useRepsList, type RepDto } from "../../../lib/reps/useReps";
import { useDeleteRep } from "../../../lib/reps/useRepMutations";
import { RepFormDialog } from "./RepFormDialog";

export function RepsPage() {
  const { has } = usePermissions();
  const { data, isLoading, isError, error, refetch } = useRepsList();

  const [createOpen, setCreateOpen] = useState(false);
  const [editing, setEditing] = useState<RepDto | null>(null);
  const [deleting, setDeleting] = useState<RepDto | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const deleteMutation = useDeleteRep();

  const canWrite = has("reps:write");
  const canDelete = has("reps:delete");

  const summary = useMemo(() => {
    if (!data) return null;
    if (data.total === 0) return "No reps yet.";
    return `${data.total} rep${data.total === 1 ? "" : "s"}`;
  }, [data]);

  return (
    <div className="space-y-6 max-w-5xl">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Sales reps</h1>
          <p className="text-sm text-muted-foreground">
            A reusable directory. Attach reps to a form, then generate a per-rep
            QR link so each scan attributes the lead to that rep.
          </p>
        </div>
        <RequirePermission perm="reps:write">
          <Button onClick={() => setCreateOpen(true)}>
            <Plus className="h-4 w-4" />
            New rep
          </Button>
        </RequirePermission>
      </div>

      {isError && (
        <Alert variant="destructive">
          <AlertTitle>Couldn't load reps</AlertTitle>
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
              <TableHead>Key</TableHead>
              <TableHead>Email</TableHead>
              <TableHead>GHL owner</TableHead>
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
                    <Skeleton className="h-4 w-24" />
                  </TableCell>
                  <TableCell>
                    <Skeleton className="h-4 w-36" />
                  </TableCell>
                  <TableCell>
                    <Skeleton className="h-4 w-20" />
                  </TableCell>
                  <TableCell />
                </TableRow>
              ))
            ) : data && data.items.length > 0 ? (
              data.items.map((r) => (
                <TableRow key={r.id}>
                  <TableCell className="font-medium flex items-center gap-2">
                    <Contact className="h-4 w-4 text-muted-foreground" />
                    {r.name}
                  </TableCell>
                  <TableCell>
                    <code className="text-xs rounded bg-muted px-1.5 py-0.5">
                      {r.key}
                    </code>
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {r.email ?? "—"}
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {r.ghl_user_id ? (
                      <code className="text-xs rounded bg-muted px-1.5 py-0.5">
                        {r.ghl_user_id}
                      </code>
                    ) : (
                      "—"
                    )}
                  </TableCell>
                  <TableCell className="text-right pr-2">
                    {(canWrite || canDelete) && (
                      <RowMenu
                        canWrite={canWrite}
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
                <TableCell
                  colSpan={5}
                  className="text-center py-10 text-sm text-muted-foreground"
                >
                  No reps yet. Click "New rep" to add one.
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

      <RepFormDialog open={createOpen} onOpenChange={setCreateOpen} />
      <RepFormDialog
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
          }
        }}
        title="Delete rep?"
        description={
          <span>
            This permanently removes{" "}
            <span className="font-medium text-foreground">{deleting?.name}</span>
            . Existing submissions attributed to this rep keep their data but lose
            the link. Forms still listing this rep simply ignore it.
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
