import { useCallback, useEffect, useMemo, useState } from "react";
import { useLocation, useNavigate, useSearchParams } from "react-router-dom";
import {
  ChevronLeft,
  ChevronRight,
  Download,
  Loader2,
  MoreHorizontal,
} from "lucide-react";
import {
  Button,
  Checkbox,
  ConfirmDialog,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  NativeSelect,
  Skeleton,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
  cn,
} from "@open-relay/ui";
import { QueryErrorAlert } from "../../../lib/api/QueryErrorAlert";
import { usePermissions } from "../../../lib/auth/usePermissions";
import { useFormSelectList } from "../../../lib/forms/useForms";
import { useRepsList } from "../../../lib/reps/useReps";
import {
  CLEARED_FILTERS,
  PAGE_SIZES,
  hasActiveFilters,
  readFilters,
  toExportQuery,
  toListQuery,
  writeFilters,
} from "../../../lib/submissions/filters";
import {
  type SubmissionDto,
  useSubmissionsList,
} from "../../../lib/submissions/useSubmissions";
import {
  useBulkDeleteSubmissions,
  useDeleteSubmission,
  useExportSubmissions,
  useRetryDeliveries,
} from "../../../lib/submissions/useSubmissionMutations";
import { DeliveryStatusBadges, DuplicateBadge } from "./DeliveryStatusBadges";
import { SubmissionDetailSheet } from "./SubmissionDetailSheet";
import { type FilterChange, SubmissionsToolbar } from "./SubmissionsToolbar";

/** History state marking an entry pushed by opening the sheet from the list. */
interface SheetHistoryState {
  submissionSheet?: boolean;
}

function submitterName(s: SubmissionDto): string | null {
  return [s.first_name, s.last_name].filter(Boolean).join(" ") || null;
}

function toggleIn(set: Set<number>, id: number): Set<number> {
  const next = new Set(set);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  return next;
}

export function SubmissionsPage() {
  const { has } = usePermissions();
  const canDelete = has("submissions:delete");
  const canRetry = has("submissions:retry");
  const canReadForms = has("forms:read");
  const canReadReps = has("reps:read");

  // Every piece of view state lives in the URL — see `lib/submissions/filters`.
  const [params, setParams] = useSearchParams();
  const location = useLocation();
  const navigate = useNavigate();
  const filters = useMemo(() => readFilters(params), [params]);
  const listQuery = useMemo(() => toListQuery(filters), [filters]);
  const listKey = JSON.stringify(listQuery);

  const updateFilters = useCallback(
    ({ patch, replace }: FilterChange) => {
      setParams((prev) => writeFilters(prev, patch), { replace });
    },
    [setParams],
  );

  const { data, isLoading, isFetching, isPlaceholderData, isError, error, refetch } =
    useSubmissionsList(listQuery);
  const items = useMemo(() => data?.items ?? [], [data]);

  // Form and rep names are enrichment only — the table falls back to
  // `Form #N`, so a reader without `forms:read` loses the label, not the row.
  const { data: forms } = useFormSelectList({ enabled: canReadForms });
  const { data: reps } = useRepsList({ enabled: canReadReps });
  const formNameById = useMemo(() => {
    const map = new Map<number, string>();
    forms?.forEach((f) => map.set(f.id, f.label));
    return map;
  }, [forms]);

  // --- Detail sheet (`?submission=ID`) ---------------------------------------
  const detailParam = params.get("submission");
  const detailId = detailParam && /^\d+$/.test(detailParam) ? Number(detailParam) : null;
  const setDetailParam = useCallback(
    (id: number | null, options: { replace?: boolean; state?: unknown }) => {
      setParams((prev) => {
        const next = new URLSearchParams(prev);
        if (id == null) next.delete("submission");
        else next.set("submission", String(id));
        return next;
      }, options);
    },
    [setParams],
  );
  const openDetail = (id: number) =>
    setDetailParam(id, { state: { submissionSheet: true } satisfies SheetHistoryState });
  // Stepping between rows replaces the entry, so Back closes the sheet rather
  // than replaying every row the admin looked at.
  const stepDetail = (id: number) =>
    setDetailParam(id, { replace: true, state: location.state });
  const closeDetail = () => {
    // Opened from the list: pop the entry we pushed, so Back doesn't reopen it.
    // Arrived by a pasted link: there is nothing of ours to pop.
    if ((location.state as SheetHistoryState | null)?.submissionSheet) navigate(-1);
    else setDetailParam(null, { replace: true });
  };
  const detailIndex = items.findIndex((s) => s.id === detailId);
  const prevId = detailIndex > 0 ? (items[detailIndex - 1]?.id ?? null) : null;
  const nextId = detailIndex >= 0 ? (items[detailIndex + 1]?.id ?? null) : null;

  // --- Selection --------------------------------------------------------------
  // Submission ids for bulk delete, and delivery ids queued for a re-sync.
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [selectedDeliveries, setSelectedDeliveries] = useState<Set<number>>(new Set());

  const exportMutation = useExportSubmissions();
  const { reset: resetExport } = exportMutation;

  // A different result set invalidates whatever was selected on the old one.
  useEffect(() => {
    setSelected(new Set());
    setSelectedDeliveries(new Set());
    resetExport();
  }, [listKey, resetExport]);

  // Deleting the last rows of the last page leaves the URL pointing past the
  // end; step back to the new last page instead of showing an empty table.
  useEffect(() => {
    if (!data || isPlaceholderData) return;
    const lastPage = Math.max(1, Math.ceil(data.total / filters.perPage));
    if (filters.page > lastPage) updateFilters({ patch: { page: lastPage }, replace: true });
  }, [data, isPlaceholderData, filters.page, filters.perPage, updateFilters]);

  const pageIds = items.map((s) => s.id);
  const selectedOnPage = pageIds.filter((id) => selected.has(id)).length;
  const allSelected = pageIds.length > 0 && selectedOnPage === pageIds.length;

  // --- Mutations --------------------------------------------------------------
  const [deleting, setDeleting] = useState<SubmissionDto | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const deleteMutation = useDeleteSubmission();

  const [bulkConfirming, setBulkConfirming] = useState(false);
  const [bulkError, setBulkError] = useState<string | null>(null);
  const bulkDeleteMutation = useBulkDeleteSubmissions();

  const [retryConfirming, setRetryConfirming] = useState(false);
  const [retryError, setRetryError] = useState<string | null>(null);
  const retryMutation = useRetryDeliveries();

  const requestDelete = (s: SubmissionDto) => {
    setDeleteError(null);
    setDeleting(s);
  };

  // --- Derived text -----------------------------------------------------------
  const pageCount = data ? Math.max(1, Math.ceil(data.total / filters.perPage)) : 1;
  const rangeText =
    data && data.total > 0
      ? `Showing ${data.offset + 1}–${Math.min(data.offset + data.items.length, data.total)} of ${data.total}`
      : null;
  const filtered = hasActiveFilters(filters);
  const columnCount = canDelete ? 6 : 5;

  return (
    <div className="space-y-6 max-w-6xl">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Submissions</h1>
          <p className="text-sm text-muted-foreground">
            Form fills delivered through OpenRelay, with per-backend status.
          </p>
        </div>
        <div className="flex items-center gap-2">
          {canRetry && selectedDeliveries.size > 0 && (
            <Button
              size="sm"
              onClick={() => {
                setRetryError(null);
                setRetryConfirming(true);
              }}
            >
              Re-sync {selectedDeliveries.size}{" "}
              {selectedDeliveries.size === 1 ? "delivery" : "deliveries"}
            </Button>
          )}
          <Button
            variant="outline"
            size="sm"
            disabled={exportMutation.isPending || data?.total === 0}
            title={filtered ? "Export every submission matching these filters" : "Export every submission"}
            onClick={() => exportMutation.mutate({ query: toExportQuery(filters) })}
          >
            {exportMutation.isPending ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Download className="h-4 w-4" />
            )}
            Export CSV
          </Button>
        </div>
      </div>

      <SubmissionsToolbar
        filters={filters}
        onChange={updateFilters}
        forms={canReadForms ? (forms ?? []) : undefined}
        reps={canReadReps ? (reps?.items ?? []) : undefined}
      />

      <QueryErrorAlert
        error={isError ? error : null}
        title="Couldn't load submissions"
        onRetry={() => refetch()}
      />
      <QueryErrorAlert error={exportMutation.error} title="Couldn't export submissions" />

      {canDelete && selected.size > 0 && (
        <div className="flex flex-wrap items-center gap-3 rounded-lg border border-border bg-muted/40 px-3 py-2 text-sm">
          <span className="font-medium">{selected.size} selected</span>
          <Button
            size="sm"
            variant="destructive"
            onClick={() => {
              setBulkError(null);
              setBulkConfirming(true);
            }}
          >
            Delete selected
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setSelected(new Set())}>
            Clear selection
          </Button>
        </div>
      )}

      <div className="border border-border rounded-lg bg-background">
        <div
          aria-busy={isFetching}
          className={cn(
            "transition-opacity",
            isFetching && isPlaceholderData && "opacity-60",
          )}
        >
          <Table>
            <TableHeader>
              <TableRow>
                {canDelete && (
                  <TableHead className="w-10 pl-3">
                    <Checkbox
                      aria-label="Select all submissions on this page"
                      checked={allSelected}
                      indeterminate={selectedOnPage > 0 && !allSelected}
                      disabled={pageIds.length === 0}
                      onChange={() =>
                        setSelected((prev) => {
                          const next = new Set(prev);
                          for (const id of pageIds) {
                            if (allSelected) next.delete(id);
                            else next.add(id);
                          }
                          return next;
                        })
                      }
                    />
                  </TableHead>
                )}
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
                    {canDelete && <TableCell />}
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
              ) : items.length > 0 ? (
                items.map((s) => {
                  const name = submitterName(s);
                  const isSelected = selected.has(s.id);
                  return (
                    <TableRow
                      key={s.id}
                      tabIndex={0}
                      data-state={isSelected ? "selected" : undefined}
                      className={cn(
                        "cursor-pointer focus-visible:bg-muted/50 focus-visible:outline-none",
                        s.id === detailId && "bg-muted/50",
                      )}
                      onClick={() => openDetail(s.id)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" && e.target === e.currentTarget) {
                          e.preventDefault();
                          openDetail(s.id);
                        }
                      }}
                    >
                      {canDelete && (
                        <TableCell className="pl-3" onClick={(e) => e.stopPropagation()}>
                          <Checkbox
                            aria-label={`Select submission #${s.id}`}
                            checked={isSelected}
                            onChange={() => setSelected((prev) => toggleIn(prev, s.id))}
                          />
                        </TableCell>
                      )}
                      <TableCell className="whitespace-nowrap text-sm">
                        <time dateTime={s.created_at} title={s.created_at}>
                          {new Date(s.created_at).toLocaleString()}
                        </time>
                        <div className="text-xs text-muted-foreground">#{s.id}</div>
                      </TableCell>
                      <TableCell className="font-medium">
                        {formNameById.get(s.form_id) ?? `Form #${s.form_id}`}
                      </TableCell>
                      <TableCell className="max-w-[16rem] text-sm">
                        <div className="truncate">{name ?? s.email ?? "—"}</div>
                        {name && s.email && (
                          <div className="truncate text-xs text-muted-foreground">
                            {s.email}
                          </div>
                        )}
                      </TableCell>
                      <TableCell
                        onClick={canRetry ? (e) => e.stopPropagation() : undefined}
                      >
                        {s.is_duplicate ? (
                          <DuplicateBadge />
                        ) : (
                          <DeliveryStatusBadges
                            deliveries={s.deliveries}
                            selectable={canRetry}
                            selectedIds={selectedDeliveries}
                            onToggle={(id) =>
                              setSelectedDeliveries((prev) => toggleIn(prev, id))
                            }
                          />
                        )}
                      </TableCell>
                      <TableCell
                        className="text-right pr-2"
                        onClick={(e) => e.stopPropagation()}
                      >
                        <DropdownMenu>
                          <DropdownMenuTrigger asChild>
                            <Button variant="ghost" size="sm" aria-label="Row actions">
                              <MoreHorizontal className="h-4 w-4" />
                            </Button>
                          </DropdownMenuTrigger>
                          <DropdownMenuContent align="end">
                            <DropdownMenuItem onSelect={() => openDetail(s.id)}>
                              View details
                            </DropdownMenuItem>
                            {canDelete && (
                              <>
                                <DropdownMenuSeparator />
                                <DropdownMenuItem
                                  onSelect={() => requestDelete(s)}
                                  className="text-destructive focus:text-destructive"
                                >
                                  Delete
                                </DropdownMenuItem>
                              </>
                            )}
                          </DropdownMenuContent>
                        </DropdownMenu>
                      </TableCell>
                    </TableRow>
                  );
                })
              ) : (
                <TableRow>
                  <TableCell
                    colSpan={columnCount}
                    className="text-center py-10 text-sm text-muted-foreground"
                  >
                    {filtered ? (
                      <div className="space-y-2">
                        <p>No submissions match these filters.</p>
                        <Button
                          variant="outline"
                          size="sm"
                          onClick={() => updateFilters({ patch: CLEARED_FILTERS })}
                        >
                          Clear filters
                        </Button>
                      </div>
                    ) : (
                      "No submissions yet."
                    )}
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </div>
        <div className="flex flex-wrap items-center justify-between gap-3 border-t border-border px-3 py-2 text-sm text-muted-foreground">
          <div>{rangeText}</div>
          <div className="flex flex-wrap items-center gap-3">
            <label className="flex items-center gap-2">
              Rows per page
              <NativeSelect
                className="h-8 w-auto"
                value={filters.perPage}
                onChange={(e) => updateFilters({ patch: { perPage: Number(e.target.value) } })}
              >
                {PAGE_SIZES.map((n) => (
                  <option key={n} value={n}>
                    {n}
                  </option>
                ))}
              </NativeSelect>
            </label>
            <span>
              Page {Math.min(filters.page, pageCount)} of {pageCount}
            </span>
            <div className="flex items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                disabled={filters.page <= 1 || isLoading}
                onClick={() => updateFilters({ patch: { page: filters.page - 1 } })}
              >
                <ChevronLeft className="h-4 w-4" />
                Prev
              </Button>
              <Button
                variant="outline"
                size="sm"
                disabled={filters.page >= pageCount || isLoading}
                onClick={() => updateFilters({ patch: { page: filters.page + 1 } })}
              >
                Next
                <ChevronRight className="h-4 w-4" />
              </Button>
            </div>
          </div>
        </div>
      </div>

      <SubmissionDetailSheet
        id={detailId}
        onClose={closeDetail}
        formNameById={formNameById}
        prevId={prevId}
        nextId={nextId}
        onNavigate={stepDetail}
        onRequestDelete={requestDelete}
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
            This permanently removes submission #{deleting?.id} and its delivery log.
            {deleteError && (
              <span className="mt-2 block text-destructive">{deleteError}</span>
            )}
          </span>
        }
        confirmLabel="Delete"
        pending={deleteMutation.isPending}
        onConfirm={() => {
          if (!deleting) return;
          const id = deleting.id;
          deleteMutation.mutate(
            { id },
            {
              onSuccess: () => {
                setDeleting(null);
                setDeleteError(null);
                setSelected((prev) => {
                  const next = new Set(prev);
                  next.delete(id);
                  return next;
                });
                if (id === detailId) closeDetail();
              },
              onError: (err) => setDeleteError(err.message),
            },
          );
        }}
      />

      <ConfirmDialog
        open={bulkConfirming}
        onOpenChange={(o) => {
          if (!o) {
            setBulkConfirming(false);
            setBulkError(null);
          }
        }}
        title={`Delete ${selected.size} ${selected.size === 1 ? "submission" : "submissions"}?`}
        description={
          <span>
            This permanently removes the selected submissions and their delivery logs.
            It can't be undone.
            {bulkError && <span className="mt-2 block text-destructive">{bulkError}</span>}
          </span>
        }
        confirmLabel="Delete"
        pending={bulkDeleteMutation.isPending}
        onConfirm={() => {
          bulkDeleteMutation.mutate(
            { ids: [...selected] },
            {
              onSuccess: () => {
                setSelected(new Set());
                setBulkConfirming(false);
                setBulkError(null);
              },
              onError: (err) => setBulkError(err.message),
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
        title={`Re-sync ${selectedDeliveries.size} ${selectedDeliveries.size === 1 ? "delivery" : "deliveries"}?`}
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
            { deliveryIds: [...selectedDeliveries] },
            {
              onSuccess: () => {
                setSelectedDeliveries(new Set());
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
