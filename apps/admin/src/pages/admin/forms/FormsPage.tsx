import { useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { ChevronLeft, ChevronRight, MoreHorizontal, Plus } from "lucide-react";
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
import { useFormsList, type FormDto } from "../../../lib/forms/useForms";
import { useDeleteForm } from "../../../lib/forms/useFormMutations";
import { FormFormDialog } from "./FormFormDialog";

const PAGE_SIZE = 25;

function countEnabledStandard(form: FormDto): number {
  return Object.values(form.standard_fields).filter((f) => f.enabled).length;
}

export function FormsPage() {
  const { has } = usePermissions();
  const navigate = useNavigate();
  const [offset, setOffset] = useState(0);
  const { data, isLoading, isError, error, refetch } = useFormsList({
    limit: PAGE_SIZE,
    offset,
  });

  const [createOpen, setCreateOpen] = useState(false);
  const [editing, setEditing] = useState<FormDto | null>(null);
  const [deleting, setDeleting] = useState<FormDto | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const deleteMutation = useDeleteForm();

  const canWrite = has("forms:write");
  const canDelete = has("forms:delete");

  const rangeText = useMemo(() => {
    if (!data) return null;
    if (data.total === 0) return "No forms yet.";
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
          <h1 className="text-2xl font-semibold tracking-tight">Forms</h1>
          <p className="text-sm text-muted-foreground">
            Define the schemas embedded by host pages.
          </p>
        </div>
        <RequirePermission perm="forms:write">
          <Button onClick={() => setCreateOpen(true)}>
            <Plus className="h-4 w-4" />
            New form
          </Button>
        </RequirePermission>
      </div>

      {isError && (
        <Alert variant="destructive">
          <AlertTitle>Couldn't load forms</AlertTitle>
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
              <TableHead>Slug</TableHead>
              <TableHead>Fields</TableHead>
              <TableHead>Updated</TableHead>
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
                    <Skeleton className="h-4 w-40" />
                  </TableCell>
                  <TableCell>
                    <Skeleton className="h-4 w-32" />
                  </TableCell>
                  <TableCell>
                    <Skeleton className="h-4 w-20" />
                  </TableCell>
                  <TableCell>
                    <Skeleton className="h-4 w-24" />
                  </TableCell>
                  <TableCell />
                </TableRow>
              ))
            ) : data && data.items.length > 0 ? (
              data.items.map((f) => (
                <TableRow key={f.id}>
                  <TableCell className="font-medium">{f.name}</TableCell>
                  <TableCell>
                    <code className="text-xs rounded bg-muted px-1.5 py-0.5">
                      {f.slug}
                    </code>
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {countEnabledStandard(f)} standard ·{" "}
                    {f.custom_fields.length} custom
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {new Date(f.updated_at).toLocaleDateString()}
                  </TableCell>
                  <TableCell className="text-right pr-2">
                    <RowMenu
                      canWrite={canWrite}
                      canDelete={canDelete}
                      onPreview={() => navigate(`/forms/${f.id}/preview`)}
                      onEdit={() => setEditing(f)}
                      onDelete={() => {
                        setDeleteError(null);
                        setDeleting(f);
                      }}
                    />
                  </TableCell>
                </TableRow>
              ))
            ) : (
              <TableRow>
                <TableCell
                  colSpan={5}
                  className="text-center py-10 text-sm text-muted-foreground"
                >
                  No forms to show.
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

      <FormFormDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
      />
      <FormFormDialog
        open={editing !== null}
        onOpenChange={(o) => !o && setEditing(null)}
        existingForm={editing}
      />
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(o) => {
          if (!o) {
            setDeleting(null);
            setDeleteError(null);
          }
        }}
        title="Delete form?"
        description={
          <span>
            This permanently removes{" "}
            <span className="font-medium text-foreground">
              {deleting?.name}
            </span>
            . Any host page still loading it via the embed SDK will see a 404.
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
  onPreview: () => void;
  onEdit: () => void;
  onDelete: () => void;
}

function RowMenu({
  canWrite,
  canDelete,
  onPreview,
  onEdit,
  onDelete,
}: RowMenuProps) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="sm" aria-label="Row actions">
          <MoreHorizontal className="h-4 w-4" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuItem onSelect={onPreview}>Preview / Test</DropdownMenuItem>
        {(canWrite || canDelete) && <DropdownMenuSeparator />}
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
