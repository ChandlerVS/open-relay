import { QueryClient } from "@tanstack/react-query";

export function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        retry: 1,
        refetchOnWindowFocus: false,
        staleTime: 30_000,
      },
      mutations: {
        retry: 0,
      },
    },
  });
}

/**
 * Options for a list query that some callers may not be allowed to run.
 *
 * Several pickers read a resource the current user might not hold `:read` on
 * (the form dialog's backends and reps lists, the role picker, the submissions
 * form filter). Passing `enabled: has("x:read")` keeps the request from being
 * fired at all, so the UI degrades to an explanation instead of rendering a
 * failed fetch.
 */
export interface GatedQueryOptions {
  enabled?: boolean;
}
