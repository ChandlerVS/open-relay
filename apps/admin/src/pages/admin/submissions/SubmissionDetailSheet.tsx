import { useEffect, useState } from "react";
import { ChevronLeft, ChevronRight, Copy, RefreshCw, Trash2 } from "lucide-react";
import { isHttpUrl } from "@open-relay/form-renderer";
import {
  Button,
  Sheet,
  SheetBody,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
  Skeleton,
} from "@open-relay/ui";
import { QueryErrorAlert } from "../../../lib/api/QueryErrorAlert";
import { usePermissions } from "../../../lib/auth/usePermissions";
import { useRepsList } from "../../../lib/reps/useReps";
import {
  type SubmissionDeliveryDto,
  type SubmissionDto,
  useSubmission,
} from "../../../lib/submissions/useSubmissions";
import { useRetryDeliveries } from "../../../lib/submissions/useSubmissionMutations";
import { DeliveryStatusChip, DuplicateBadge } from "./DeliveryStatusBadges";

interface Props {
  /** The open submission, or `null` for a closed sheet. */
  id: number | null;
  onClose: () => void;
  formNameById: Map<number, string>;
  /** Neighbours on the current list page; `null` at either edge. */
  prevId: number | null;
  nextId: number | null;
  onNavigate: (id: number) => void;
  onRequestDelete: (submission: SubmissionDto) => void;
}

const STANDARD_KEYS = [
  ["first_name", "First name"],
  ["last_name", "Last name"],
  ["email", "Email"],
  ["phone", "Phone"],
  ["company", "Company"],
  ["job_title", "Job title"],
  ["website", "Website"],
  ["message", "Message"],
  ["address_line_1", "Address line 1"],
  ["address_line_2", "Address line 2"],
  ["city", "City"],
  ["state", "State / region"],
  ["postal_code", "Postal code"],
  ["country", "Country"],
] as const;

const FAILED_STATUSES = new Set(["permanent_failure", "exhausted"]);

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

export function SubmissionDetailSheet({
  id,
  onClose,
  formNameById,
  prevId,
  nextId,
  onNavigate,
  onRequestDelete,
}: Props) {
  const { has } = usePermissions();
  const canRetry = has("submissions:retry");
  const canDelete = has("submissions:delete");
  // `useSubmission` rather than the list row, so a pasted `?submission=` link
  // to a row on another page (or filtered out) still opens.
  const { data, isLoading, error, refetch } = useSubmission(id);
  // Rep names are enrichment only — the `Rep #N` fallback covers a reader
  // without `reps:read`.
  const { data: reps } = useRepsList({ enabled: has("reps:read") });
  const retry = useRetryDeliveries();
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    setNotice(null);
  }, [id]);

  const repName =
    data?.sales_rep_id != null
      ? (reps?.items.find((r) => r.id === data.sales_rep_id)?.name ??
        `Rep #${data.sales_rep_id}`)
      : null;
  const sourceParams = asRecord(data?.source_params);
  const custom = asRecord(data?.custom_data);
  const hasAttribution =
    repName != null || (sourceParams != null && Object.keys(sourceParams).length > 0);
  const failedIds =
    data?.deliveries.filter((d) => FAILED_STATUSES.has(d.status)).map((d) => d.id) ?? [];

  const copyJson = async () => {
    if (!data) return;
    try {
      await navigator.clipboard.writeText(JSON.stringify(data, null, 2));
      setNotice("Copied as JSON.");
    } catch {
      setNotice("Couldn't copy — the browser blocked clipboard access.");
    }
  };

  return (
    <Sheet open={id !== null} onOpenChange={(open) => !open && onClose()}>
      <SheetContent
        onKeyDown={(e) => {
          // Step through the list without leaving the sheet — but never while
          // the admin is selecting text or typing somewhere.
          if (e.altKey || e.metaKey || e.ctrlKey) return;
          if (e.target instanceof HTMLElement && e.target.closest("input, textarea, select")) {
            return;
          }
          if ((e.key === "ArrowUp" || e.key === "k") && prevId != null) {
            e.preventDefault();
            onNavigate(prevId);
          } else if ((e.key === "ArrowDown" || e.key === "j") && nextId != null) {
            e.preventDefault();
            onNavigate(nextId);
          }
        }}
      >
        <SheetHeader className="pr-14">
          <div className="flex items-start justify-between gap-3">
            <div className="min-w-0 space-y-1.5">
              <SheetTitle>Submission #{data?.id ?? id}</SheetTitle>
              <SheetDescription>
                {data
                  ? `${formNameById.get(data.form_id) ?? `Form #${data.form_id}`} · ${new Date(data.created_at).toLocaleString()}`
                  : "Loading…"}
              </SheetDescription>
            </div>
            <div className="flex shrink-0 items-center gap-1">
              <Button
                variant="outline"
                size="sm"
                aria-label="Previous submission"
                title="Previous (↑ or k)"
                disabled={prevId == null}
                onClick={() => prevId != null && onNavigate(prevId)}
              >
                <ChevronLeft className="h-4 w-4" />
              </Button>
              <Button
                variant="outline"
                size="sm"
                aria-label="Next submission"
                title="Next (↓ or j)"
                disabled={nextId == null}
                onClick={() => nextId != null && onNavigate(nextId)}
              >
                <ChevronRight className="h-4 w-4" />
              </Button>
            </div>
          </div>
        </SheetHeader>

        <SheetBody>
          <QueryErrorAlert
            error={error}
            title="Couldn't load submission"
            onRetry={() => refetch()}
          />
          {error ? null : isLoading || !data ? (
            <div className="space-y-2">
              <Skeleton className="h-4 w-full" />
              <Skeleton className="h-4 w-3/4" />
              <Skeleton className="h-4 w-1/2" />
            </div>
          ) : (
            <div className="space-y-6">
              <section>
                <h3 className="mb-2 text-sm font-medium">Delivery</h3>
                {data.is_duplicate ? (
                  <div className="space-y-1">
                    <DuplicateBadge />
                    <p className="text-xs text-muted-foreground">
                      Accepted as a duplicate email — not delivered to any backend.
                    </p>
                  </div>
                ) : data.deliveries.length === 0 ? (
                  <p className="text-sm text-muted-foreground">
                    No backends were bound to this form when it was submitted.
                  </p>
                ) : (
                  <ul className="space-y-2">
                    {data.deliveries.map((d) => (
                      <DeliveryRow key={d.id} delivery={d} />
                    ))}
                  </ul>
                )}
              </section>

              {hasAttribution && (
                <section>
                  <h3 className="mb-2 text-sm font-medium">Attribution</h3>
                  <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
                    {repName && (
                      <div>
                        <dt className="text-xs text-muted-foreground">Sales rep</dt>
                        <dd className="break-words">{repName}</dd>
                      </div>
                    )}
                    {sourceParams &&
                      Object.entries(sourceParams).map(([key, value]) => (
                        <div key={key}>
                          <dt className="text-xs text-muted-foreground">{key}</dt>
                          <dd className="break-words">{String(value)}</dd>
                        </div>
                      ))}
                  </dl>
                </section>
              )}

              <section>
                <h3 className="mb-2 text-sm font-medium">Standard fields</h3>
                <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
                  {STANDARD_KEYS.map(([key, label]) => {
                    const value = (data as unknown as Record<string, unknown>)[key];
                    if (value == null || value === "") return null;
                    return (
                      <div key={key} className={key === "message" ? "col-span-2" : undefined}>
                        <dt className="text-xs text-muted-foreground">{label}</dt>
                        <dd className="whitespace-pre-line break-words">{String(value)}</dd>
                      </div>
                    );
                  })}
                </dl>
              </section>

              {custom && Object.keys(custom).length > 0 && (
                <section>
                  <h3 className="mb-2 text-sm font-medium">Custom fields</h3>
                  <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
                    {Object.entries(custom).map(([key, value]) => (
                      <div key={key}>
                        <dt className="text-xs text-muted-foreground">{key}</dt>
                        <dd className="break-words">
                          <CustomValue value={value} />
                        </dd>
                      </div>
                    ))}
                  </dl>
                </section>
              )}
            </div>
          )}
        </SheetBody>

        {data && (
          <SheetFooter className="justify-between">
            <p className="min-w-0 truncate text-xs text-muted-foreground" role="status">
              {notice}
            </p>
            <div className="flex shrink-0 items-center gap-2">
              <Button variant="outline" size="sm" onClick={copyJson}>
                <Copy className="h-4 w-4" />
                Copy JSON
              </Button>
              {canRetry && failedIds.length > 0 && (
                <Button
                  variant="outline"
                  size="sm"
                  disabled={retry.isPending}
                  onClick={() =>
                    retry.mutate(
                      { deliveryIds: failedIds },
                      {
                        onSuccess: (r) =>
                          setNotice(
                            `Re-queued ${r.requeued.length} ${r.requeued.length === 1 ? "delivery" : "deliveries"}.`,
                          ),
                        onError: (err) => setNotice(err.message),
                      },
                    )
                  }
                >
                  <RefreshCw className={retry.isPending ? "h-4 w-4 animate-spin" : "h-4 w-4"} />
                  Re-sync failed
                </Button>
              )}
              {canDelete && (
                <Button variant="destructive" size="sm" onClick={() => onRequestDelete(data)}>
                  <Trash2 className="h-4 w-4" />
                  Delete
                </Button>
              )}
            </div>
          </SheetFooter>
        )}
      </SheetContent>
    </Sheet>
  );
}

/**
 * The tooltip-only `last_error` on the list chips is fine for scanning; here,
 * where someone is diagnosing a failure, it is printed in full.
 */
function DeliveryRow({ delivery: d }: { delivery: SubmissionDeliveryDto }) {
  const queued = d.status === "pending" || d.status === "in_progress";
  const when = d.delivered_at
    ? `delivered ${new Date(d.delivered_at).toLocaleString()}`
    : queued
      ? `next attempt ${new Date(d.next_attempt_at).toLocaleString()}`
      : `last attempt ${new Date(d.updated_at).toLocaleString()}`;
  return (
    <li className="rounded-md border border-border px-3 py-2">
      <div className="flex items-center justify-between gap-2 text-sm">
        <span className="truncate font-medium">{d.backend_name}</span>
        <DeliveryStatusChip status={d.status} />
      </div>
      <p className="mt-0.5 text-xs text-muted-foreground">
        {d.attempts} {d.attempts === 1 ? "attempt" : "attempts"} · {when}
      </p>
      {d.last_error && (
        <pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap break-words rounded bg-destructive/5 px-2 py-1.5 font-mono text-xs text-destructive">
          {d.last_error}
        </pre>
      )}
    </li>
  );
}

/**
 * A file field stores the uploaded object's URL, so render it as a link rather
 * than a wall of unclickable text. Any other http(s) value benefits equally.
 *
 * `noopener noreferrer` because the URL points at a bucket we don't control,
 * and the value ultimately came from a form submission.
 */
function CustomValue({ value }: { value: unknown }) {
  // A checkbox group stores the ticked options as an array.
  if (Array.isArray(value)) return <>{value.map(String).join(", ")}</>;
  const text = typeof value === "string" ? value : JSON.stringify(value);
  if (typeof value === "string" && isHttpUrl(value)) {
    return (
      <a
        href={text}
        target="_blank"
        rel="noopener noreferrer"
        className="text-primary underline underline-offset-2"
      >
        {text}
      </a>
    );
  }
  return <>{text}</>;
}
