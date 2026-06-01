import { createApiClient } from "@open-relay/api-client";
import {
  emitSessionExpired,
  getCurrentRefreshToken,
  getCurrentToken,
  storeTokens,
} from "./tokenSource";

const baseUrl = import.meta.env.VITE_API_URL ?? "http://localhost:8080";

// Single-flight refresh: concurrent 401s share one in-flight refresh so we
// don't rotate the refresh token several times in parallel (which would
// invalidate all but one and log the user out).
let refreshInFlight: Promise<boolean> | null = null;

async function doRefresh(): Promise<boolean> {
  const refreshToken = getCurrentRefreshToken();
  if (!refreshToken) return false;
  try {
    const res = await fetch(`${baseUrl}/auth/refresh`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ refresh_token: refreshToken }),
    });
    if (!res.ok) return false;
    const data = (await res.json()) as { token: string; refresh_token: string };
    storeTokens(data.token, data.refresh_token);
    return true;
  } catch {
    return false;
  }
}

function runRefresh(): Promise<boolean> {
  if (!refreshInFlight) {
    refreshInFlight = doRefresh().finally(() => {
      refreshInFlight = null;
    });
  }
  return refreshInFlight;
}

/**
 * Custom fetch wrapping the global one with transparent access-token refresh.
 * On a 401 (for an authenticated request that isn't itself `/auth/refresh`) it
 * runs a single-flight refresh and retries the original request once with the
 * new access token; if the refresh fails the session is expired.
 */
async function fetchWithRefresh(input: Request): Promise<Response> {
  // Clone up front so we retain a usable copy (incl. body) for the retry.
  const retryable = input.clone();
  const res = await fetch(input);
  if (res.status !== 401) return res;
  // Only attempt refresh for authenticated requests; never recurse on refresh.
  if (!getCurrentToken() || retryable.url.endsWith("/auth/refresh")) return res;

  const refreshed = await runRefresh();
  if (!refreshed) {
    emitSessionExpired();
    return res;
  }
  const retry = new Request(retryable, { headers: new Headers(retryable.headers) });
  retry.headers.set("Authorization", `Bearer ${getCurrentToken()}`);
  return fetch(retry);
}

const client = createApiClient({ baseUrl, fetch: fetchWithRefresh });

client.use({
  onRequest({ request }) {
    const token = getCurrentToken();
    if (token) request.headers.set("Authorization", `Bearer ${token}`);
  },
});

export const api = {
  baseUrl,
  client,
};

export type Api = typeof api;
