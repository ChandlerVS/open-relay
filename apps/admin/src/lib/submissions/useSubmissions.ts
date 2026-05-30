import { useQuery } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { extractApiErrorMessage } from "../api/errors";

export type SubmissionDto = components["schemas"]["SubmissionDto"];
export type SubmissionList = components["schemas"]["SubmissionList"];
export type SubmissionDeliveryDto = components["schemas"]["SubmissionDeliveryDto"];

export interface SubmissionsListParams {
  formId?: number;
  limit?: number;
  offset?: number;
}

export function useSubmissionsList(params: SubmissionsListParams = {}) {
  const { formId, limit, offset } = params;
  return useQuery<SubmissionList>({
    queryKey: ["submissions", "list", { formId, limit, offset }],
    queryFn: async () => {
      const query: Record<string, number> = {};
      if (typeof formId === "number") query.form_id = formId;
      if (typeof limit === "number") query.limit = limit;
      if (typeof offset === "number") query.offset = offset;
      const { data, error } = await api.client.GET("/submissions", {
        params: { query },
      });
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Failed to load submissions."));
    },
    staleTime: 10_000,
  });
}

export function useSubmission(id: number | null) {
  return useQuery<SubmissionDto>({
    queryKey: ["submissions", "detail", id],
    enabled: id != null,
    queryFn: async () => {
      const { data, error } = await api.client.GET("/submissions/{id}", {
        params: { path: { id: id as number } },
      });
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Failed to load submission."));
    },
  });
}
