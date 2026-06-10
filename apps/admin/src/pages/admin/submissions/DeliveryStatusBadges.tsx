import { cn } from "@open-relay/ui";
import type { SubmissionDeliveryDto } from "../../../lib/submissions/useSubmissions";

const STATUS_CLASS: Record<string, string> = {
  succeeded: "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
  pending: "bg-amber-500/10 text-amber-700 dark:text-amber-300",
  in_progress: "bg-sky-500/10 text-sky-700 dark:text-sky-300",
  permanent_failure: "bg-destructive/10 text-destructive",
  exhausted: "bg-destructive/10 text-destructive",
};

const STATUS_LABEL: Record<string, string> = {
  succeeded: "delivered",
  pending: "pending",
  in_progress: "in flight",
  permanent_failure: "failed",
  exhausted: "exhausted",
};

/// A delivery can be manually re-synced only from a terminal state — a
/// `pending`/`in_progress` row is already queued, so re-syncing it is a no-op.
function isRetryable(status: string): boolean {
  return (
    status === "succeeded" ||
    status === "permanent_failure" ||
    status === "exhausted"
  );
}

interface Props {
  deliveries: SubmissionDeliveryDto[];
  /// When true, retryable chips become selection toggles.
  selectable?: boolean;
  selectedIds?: Set<number>;
  onToggle?: (id: number) => void;
}

export function DeliveryStatusBadges({
  deliveries,
  selectable = false,
  selectedIds,
  onToggle,
}: Props) {
  if (deliveries.length === 0) {
    return <span className="text-xs text-muted-foreground">—</span>;
  }
  return (
    <div className="flex flex-wrap gap-1">
      {deliveries.map((d) => {
        const base = cn(
          "inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium",
          STATUS_CLASS[d.status] ?? "bg-muted text-muted-foreground",
        );
        const label = `${d.backend_name}: ${STATUS_LABEL[d.status] ?? d.status}`;

        if (selectable && isRetryable(d.status)) {
          const selected = selectedIds?.has(d.id) ?? false;
          return (
            <button
              key={d.id}
              type="button"
              onClick={() => onToggle?.(d.id)}
              aria-pressed={selected}
              title={d.last_error ?? "Select to re-sync"}
              className={cn(
                base,
                "cursor-pointer transition-shadow hover:opacity-90",
                selected &&
                  "ring-2 ring-primary ring-offset-1 ring-offset-background",
              )}
            >
              <span
                aria-hidden
                className={cn(
                  "inline-block h-2 w-2 rounded-full border",
                  selected
                    ? "border-primary bg-primary"
                    : "border-current opacity-50",
                )}
              />
              {label}
            </button>
          );
        }

        return (
          <span key={d.id} className={base} title={d.last_error ?? undefined}>
            {label}
          </span>
        );
      })}
    </div>
  );
}
