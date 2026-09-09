import { useQuery } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { throwApiError } from "../api/errors";
import type { GatedQueryOptions } from "../api/queryClient";

export type BackendInstanceDto = components["schemas"]["BackendInstanceDto"];
export type BackendInstanceList = components["schemas"]["BackendInstanceList"];
export type BackendKindInfo = components["schemas"]["BackendKindInfo"];

export function useBackendsList({ enabled = true }: GatedQueryOptions = {}) {
  return useQuery<BackendInstanceList>({
    queryKey: ["backends", "list"],
    enabled,
    queryFn: async () => {
      const { data, error, response } = await api.client.GET("/backends");
      if (data) return data;
      throwApiError(error, response, "Failed to load backends.");
    },
    staleTime: 30_000,
  });
}

export function useBackend(id: number | null) {
  return useQuery<BackendInstanceDto>({
    queryKey: ["backends", "detail", id],
    enabled: id != null,
    queryFn: async () => {
      const { data, error, response } = await api.client.GET("/backends/{id}", {
        params: { path: { id: id as number } },
      });
      if (data) return data;
      throwApiError(error, response, "Failed to load backend.");
    },
  });
}

export function useBackendKinds() {
  return useQuery<BackendKindInfo[]>({
    queryKey: ["backends", "kinds"],
    queryFn: async () => {
      const { data, error, response } = await api.client.GET("/backends/kinds");
      if (data) return data;
      throwApiError(error, response, "Failed to load backend kinds.");
    },
    staleTime: 5 * 60_000,
  });
}
