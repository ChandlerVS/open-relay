import { useEffect, useMemo, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { ChevronLeft, ChevronRight, MoreHorizontal } from "lucide-react";
import {
  Alert,
  AlertDescription,
  AlertTitle,
  Button,
  ConfirmDialog,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  Skeleton,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@open-relay/ui";
import { usePermissions } from "../../../lib/auth/usePermissions";
import { useFormSelectList } from "../../../lib/forms/useForms";
import {
  type SubmissionDto,
  useSubmissionsList,
} from "../../../lib/submissions/useSubmissions";
import {
  useDeleteSubmission,
  useRetryDeliveries,
} from "../../../lib/submissions/useSubmissionMutations";
import { SubmissionDetailDialog } from "./SubmissionDetailDialog";
import { DeliveryStatusBadges } from "./DeliveryStatusBadges";

const PAGE_SIZE = 25;

function submitterLabel(s: SubmissionDto): string {
  if (s.email) return s.email;
  const name = [s.first_name, s.last_name].filter(Boolean).join(" ");
  return name || "—";
}

export function SubmissionsPage() {
  const { has } = usePermissions();
  const [params, setParams] = useSearchParams();
  const formIdParam = params.get("form_id");
  const formId = formIdParam ? Number(formIdParam) : undefined;
  const [offset, setOffset] = useState(0);
  const { data, isLoading, isError, error, refetch } = useSubmissionsList({
    formId: Number.isFinite(formId) ? formId : undefined,
    limit: PAGE_SIZE,
    offset,
  });
  const { data: forms } = useFormSelectList();
  const formNameById = useMemo(() => {
    const map = new Map<number, string>();
    forms?.forEach((f) => map.set(f.id, f.label));
    return map;
  }, [forms]);

  const [detailId, setDetailId] = useState<number | null>(null);
  const [deleting, setDeleting] = useState<SubmissionDto | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const deleteMutation = useDeleteSubmission();

  // Selected `submission_delivery` ids queued for a manual re-sync.
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [retryConfirming, setRetryConfirming] = useState(false);
  const [retryError, setRetryError] = useState<string | null>(null);
  const retryMutation = useRetryDeliveries();

  const canDelete = has("submissions:delete");
  const canRetry = has("submissions:retry");

  // Drop the selection whenever the visible rows change (paging / filtering),
  // since the selected delivery ids may no longer be on screen.
  useEffect(() => {
    setSelected(new Set());
  }, [offset, formId]);

  const toggleDelivery = (id: number) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const rangeText = useMemo(() => {
    if (!data) return null;
    if (data.total === 0) return "No submissions yet.";
    const start = data.offset + 1;
    const end = Math.min(data.offset + data.items.length, data.total);
    return `Showing ${start}–${end} of ${data.total}`;
  }, [data]);

  const canPrev = offset > 0;
  const canNext = data ? offset + data.items.length < data.total : false;

  return (
    <div className="space-y-6 max-w-6xl">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Submissions</h1>
          <p className="text-sm text-muted-foreground">
            Form fills delivered through OpenRelay, with per-backend status.
          </p>
        </div>
        <div className="flex items-center gap-2">
          {canRetry && selected.size > 0 && (
            <Button
              size="sm"
              onClick={() => {
                setRetryError(null);
                setRetryConfirming(true);
              }}
            >
              Re-sync {selected.size}{" "}
              {selected.size === 1 ? "delivery" : "deliveries"}
            </Button>
          )}
          {formId != null && (
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                params.delete("form_id");
                setParams(params);
                setOffset(0);
              }}
            >
              Clear form filter
            </Button>
          )}
        </div>
      </div>

      {isError && (
        <Alert variant="destructive">
          <AlertTitle>Couldn't load submissions</AlertTitle>
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
              <TableHead>Received</TableHead>
              <TableHead>Form</TableHead>
              <TableHead>Submitter</TableHead>
              <TableHead>Delivery</TableHead>
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
                    <Skeleton className="h-4 w-40" />
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
              data.items.map((s) => (
                <TableRow
                  key={s.id}
                  className="cursor-pointer"
                  onClick={() => setDetailId(s.id)}
                >
                  <TableCell className="text-sm text-muted-foreground">
                    {new Date(s.created_at).toLocaleString()}
                  </TableCell>
                  <TableCell className="font-medium">
                    {formNameById.get(s.form_id) ?? `Form #${s.form_id}`}
                  </TableCell>
                  <TableCell className="text-sm">{submitterLabel(s)}</TableCell>
                  <TableCell
                    onClick={canRetry ? (e) => e.stopPropagation() : undefined}
                  >
                    <DeliveryStatusBadges
                      deliveries={s.deliveries}
                      selectable={canRetry}
                      selectedIds={selected}
                      onToggle={toggleDelivery}
                    />
                  </TableCell>
                  <TableCell
                    className="text-right pr-2"
                    onClick={(e) => e.stopPropagation()}
                  >
                    {canDelete && (
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button
                            variant="ghost"
                            size="sm"
                            aria-label="Row actions"
                          >
                            <MoreHorizontal className="h-4 w-4" />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          <DropdownMenuItem
                            onSelect={() => {
                              setDeleteError(null);
                              setDeleting(s);
                            }}
                            className="text-destructive focus:text-destructive"
                          >
                            Delete
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
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
                  No submissions to show.
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

      <SubmissionDetailDialog
        id={detailId}
        open={detailId !== null}
        onOpenChange={(o) => !o && setDetailId(null)}
        formNameById={formNameById}
      />

      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(o) => {
          if (!o) {
            setDeleting(null);
            setDeleteError(null);
          }
        }}
        title="Delete submission?"
        description={
          <span>
            This permanently removes submission #{deleting?.id} and its delivery
            log.
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

      <ConfirmDialog
        open={retryConfirming}
        onOpenChange={(o) => {
          if (!o) {
            setRetryConfirming(false);
            setRetryError(null);
          }
        }}
        title={`Re-sync ${selected.size} ${selected.size === 1 ? "delivery" : "deliveries"}?`}
        description={
          <span>
            Re-queues the selected deliveries for immediate re-delivery to their
            backends. Already-queued or in-flight deliveries are left untouched.
            {retryError && (
              <span className="mt-2 block text-destructive">{retryError}</span>
            )}
          </span>
        }
        confirmLabel="Re-sync"
        pending={retryMutation.isPending}
        onConfirm={() => {
          retryMutation.mutate(
            { deliveryIds: [...selected] },
            {
              onSuccess: () => {
                setSelected(new Set());
                setRetryConfirming(false);
                setRetryError(null);
              },
              onError: (err) => setRetryError(err.message),
            },
          );
        }}
      />
    </div>
  );
}
