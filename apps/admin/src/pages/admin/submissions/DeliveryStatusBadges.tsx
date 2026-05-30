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

interface Props {
  deliveries: SubmissionDeliveryDto[];
}

export function DeliveryStatusBadges({ deliveries }: Props) {
  if (deliveries.length === 0) {
    return <span className="text-xs text-muted-foreground">—</span>;
  }
  return (
    <div className="flex flex-wrap gap-1">
      {deliveries.map((d) => (
        <span
          key={d.id}
          className={cn(
            "inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium",
            STATUS_CLASS[d.status] ?? "bg-muted text-muted-foreground",
          )}
          title={d.last_error ?? undefined}
        >
          {d.backend_name}: {STATUS_LABEL[d.status] ?? d.status}
        </span>
      ))}
    </div>
  );
}
