import { useQuery } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { throwApiError } from "../api/errors";
import type { GatedQueryOptions } from "../api/queryClient";

export type RepDto = components["schemas"]["RepDto"];
export type RepList = components["schemas"]["RepList"];

export function useRepsList({ enabled = true }: GatedQueryOptions = {}) {
  return useQuery<RepList>({
    queryKey: ["reps", "list"],
    enabled,
    queryFn: async () => {
      const { data, error, response } = await api.client.GET("/reps");
      if (data) return data;
      throwApiError(error, response, "Failed to load reps.");
    },
    staleTime: 30_000,
  });
}

export function useRep(id: number | null) {
  return useQuery<RepDto>({
    queryKey: ["reps", "detail", id],
    enabled: id != null,
    queryFn: async () => {
      const { data, error, response } = await api.client.GET("/reps/{id}", {
        params: { path: { id: id as number } },
      });
      if (data) return data;
      throwApiError(error, response, "Failed to load rep.");
    },
  });
}
