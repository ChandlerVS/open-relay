import { Alert, AlertDescription, AlertTitle } from "@open-relay/ui";
import { isForbidden } from "./errors";

export interface QueryErrorAlertProps {
  /** The query's `error`. Nothing renders when it is nullish. */
  error: unknown;
  /** Headline for an ordinary failure, e.g. "Couldn't load forms". */
  title: string;
  /** Retry hook — omitted from the 403 branch, where retrying cannot help. */
  onRetry?: () => void;
}

/**
 * The standard "this query failed" alert, with one distinction baked in: a 403
 * is not a fault to retry. Route guards and `enabled:` gates mean the UI
 * normally never issues a request the caller can't make, so a 403 arriving
 * here means the permission was revoked mid-session — between the last
 * `/auth/me` and this request. Offering "Try again" for that is a dead end, so
 * the forbidden branch explains instead and drops the button.
 */
export function QueryErrorAlert({ error, title, onRetry }: QueryErrorAlertProps) {
  if (!error) return null;

  if (isForbidden(error)) {
    return (
      <Alert>
        <AlertTitle>You don't have access</AlertTitle>
        <AlertDescription>
          {(error as Error).message} Your permissions may have changed since you
          signed in — reload the page to pick up the current set.
        </AlertDescription>
      </Alert>
    );
  }

  return (
    <Alert variant="destructive">
      <AlertTitle>{title}</AlertTitle>
      <AlertDescription>
        {(error as Error | undefined)?.message ?? "Unknown error."}
        {onRetry && (
          <>
            {" "}
            <button
              type="button"
              className="underline font-medium"
              onClick={onRetry}
            >
              Try again
            </button>
          </>
        )}
      </AlertDescription>
    </Alert>
  );
}
