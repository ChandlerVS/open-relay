import { keepPreviousData, useQuery } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { throwApiError } from "../api/errors";
import type { ListApiQuery } from "./filters";

export type SubmissionDto = components["schemas"]["SubmissionDto"];
export type SubmissionList = components["schemas"]["SubmissionList"];
export type SubmissionDeliveryDto = components["schemas"]["SubmissionDeliveryDto"];

/**
 * One page of submissions for `query` (build it with `toListQuery`). The
 * previous page stays on screen while the next loads, so paging and typing in
 * the search box dim the table instead of flashing skeleton rows.
 */
export function useSubmissionsList(query: ListApiQuery) {
  return useQuery<SubmissionList>({
    queryKey: ["submissions", "list", query],
    queryFn: async () => {
      const { data, error, response } = await api.client.GET("/submissions", {
        params: { query },
      });
      if (data) return data;
      throwApiError(error, response, "Failed to load submissions.");
    },
    placeholderData: keepPreviousData,
    staleTime: 10_000,
  });
}

export function useSubmission(id: number | null) {
  return useQuery<SubmissionDto>({
    queryKey: ["submissions", "detail", id],
    enabled: id != null,
    queryFn: async () => {
      const { data, error, response } = await api.client.GET("/submissions/{id}", {
        params: { path: { id: id as number } },
      });
      if (data) return data;
      throwApiError(error, response, "Failed to load submission.");
    },
  });
}
