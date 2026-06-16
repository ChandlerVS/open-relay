import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  Skeleton,
} from "@open-relay/ui";
import { useSubmission } from "../../../lib/submissions/useSubmissions";
import { useRepsList } from "../../../lib/reps/useReps";
import { DeliveryStatusBadges, DuplicateBadge } from "./DeliveryStatusBadges";

interface Props {
  id: number | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  formNameById: Map<number, string>;
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

export function SubmissionDetailDialog({
  id,
  open,
  onOpenChange,
  formNameById,
}: Props) {
  const { data, isLoading } = useSubmission(id);
  const { data: reps } = useRepsList();
  const repName =
    data?.sales_rep_id != null
      ? (reps?.items.find((r) => r.id === data.sales_rep_id)?.name ??
        `Rep #${data.sales_rep_id}`)
      : null;
  const sourceParams =
    data?.source_params &&
    typeof data.source_params === "object" &&
    !Array.isArray(data.source_params)
      ? (data.source_params as Record<string, unknown>)
      : null;
  const hasAttribution =
    repName != null || (sourceParams != null && Object.keys(sourceParams).length > 0);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>
            {data
              ? `Submission #${data.id}`
              : id !== null
                ? `Submission #${id}`
                : "Submission"}
          </DialogTitle>
          <DialogDescription>
            {data
              ? `${formNameById.get(data.form_id) ?? `Form #${data.form_id}`} · ${new Date(data.created_at).toLocaleString()}`
              : "Loading…"}
          </DialogDescription>
        </DialogHeader>

        {isLoading || !data ? (
          <div className="space-y-2">
            <Skeleton className="h-4 w-full" />
            <Skeleton className="h-4 w-3/4" />
            <Skeleton className="h-4 w-1/2" />
          </div>
        ) : (
          <div className="space-y-6">
            <section>
              <h3 className="text-sm font-medium mb-2">Delivery</h3>
              {data.is_duplicate ? (
                <div className="space-y-1">
                  <DuplicateBadge />
                  <p className="text-xs text-muted-foreground">
                    Accepted as a duplicate email — not delivered to any
                    backend.
                  </p>
                </div>
              ) : (
                <DeliveryStatusBadges deliveries={data.deliveries} />
              )}
            </section>
            {hasAttribution && (
              <section>
                <h3 className="text-sm font-medium mb-2">Attribution</h3>
                <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
                  {repName && (
                    <div>
                      <dt className="text-muted-foreground text-xs">Sales rep</dt>
                      <dd className="break-words">{repName}</dd>
                    </div>
                  )}
                  {sourceParams &&
                    Object.entries(sourceParams).map(([key, value]) => (
                      <div key={key}>
                        <dt className="text-muted-foreground text-xs">{key}</dt>
                        <dd className="break-words">{String(value)}</dd>
                      </div>
                    ))}
                </dl>
              </section>
            )}
            <section>
              <h3 className="text-sm font-medium mb-2">Standard fields</h3>
              <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
                {STANDARD_KEYS.map(([key, label]) => {
                  const value = (data as unknown as Record<string, unknown>)[key];
                  if (value == null || value === "") return null;
                  return (
                    <div key={key}>
                      <dt className="text-muted-foreground text-xs">{label}</dt>
                      <dd className="break-words">{String(value)}</dd>
                    </div>
                  );
                })}
              </dl>
            </section>
            {(() => {
              const custom =
                data.custom_data &&
                typeof data.custom_data === "object" &&
                !Array.isArray(data.custom_data)
                  ? (data.custom_data as Record<string, unknown>)
                  : null;
              if (!custom || Object.keys(custom).length === 0) return null;
              return (
                <section>
                  <h3 className="text-sm font-medium mb-2">Custom fields</h3>
                  <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
                    {Object.entries(custom).map(([key, value]) => (
                      <div key={key}>
                        <dt className="text-muted-foreground text-xs">{key}</dt>
                        <dd className="break-words">{String(value)}</dd>
                      </div>
                    ))}
                  </dl>
                </section>
              );
            })()}
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
