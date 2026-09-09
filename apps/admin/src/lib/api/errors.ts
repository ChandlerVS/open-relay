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

/**
 * An API failure that remembers its HTTP status.
 *
 * The status matters for exactly one distinction the UI has to draw: a 403 is
 * a permission problem, and retrying it will never succeed, so it must not be
 * rendered as the generic "Couldn't load X — Try again" alert every other
 * failure gets. Everything else stays an ordinary `Error` as far as callers
 * are concerned — `message` is still the server's `{ error }` string.
 */
export class ApiError extends Error {
  readonly status: number;

  constructor(message: string, status: number) {
    super(message);
    this.name = "ApiError";
    this.status = status;
  }
}

/**
 * Throw the `ApiError` for a non-2xx openapi-fetch result. Always throws; the
 * `never` return lets a query function end with `throw throwApiError(...)` or
 * plain `throwApiError(...)` without upsetting the narrowing on `data`.
 */
export function throwApiError(
  error: unknown,
  response: Response,
  fallback: string,
): never {
  throw new ApiError(extractApiErrorMessage(error, fallback), response.status);
}

/**
 * True when a query/mutation failed because the caller lacks the permission
 * the route requires. Route guards and `enabled:` gates mean this should
 * rarely fire — it is the backstop for a permission revoked mid-session,
 * between the last `/auth/me` and the request.
 */
export function isForbidden(error: unknown): boolean {
  return error instanceof ApiError && error.status === 403;
}
