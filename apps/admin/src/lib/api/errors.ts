/**
 * Backend errors are JSON `{ error: string }`. openapi-fetch returns the
 * parsed body in `response.error` when status is non-2xx, but the schema
 * declares those responses as `content?: never`, so the body is typed as
 * `unknown`. This narrows it back to a useful string.
 */
export function extractApiErrorMessage(error: unknown, fallback: string): string {
  if (error && typeof error === "object" && "error" in error) {
    const msg = (error as { error: unknown }).error;
    if (typeof msg === "string" && msg.length > 0) return msg;
  }
  return fallback;
}
