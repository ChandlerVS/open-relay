import { useQuery } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { throwApiError } from "../api/errors";
import type { GatedQueryOptions } from "../api/queryClient";

export type RoleDto = components["schemas"]["RoleDto"];
export type RoleSummary = components["schemas"]["RoleSummary"];

export function useRolesList() {
  return useQuery<RoleDto[]>({
    queryKey: ["roles", "list"],
    queryFn: async () => {
      const { data, error, response } = await api.client.GET("/roles");
      if (data) return data;
      throwApiError(error, response, "Failed to load roles.");
    },
    staleTime: 30_000,
  });
}

export function useRole(id: number | null) {
  return useQuery<RoleDto>({
    queryKey: ["roles", "detail", id],
    enabled: id != null,
    queryFn: async () => {
      const { data, error, response } = await api.client.GET("/roles/{id}", {
        params: { path: { id: id as number } },
      });
      if (data) return data;
      throwApiError(error, response, "Failed to load role.");
    },
  });
}

export function useRoleSelectList({ enabled = true }: GatedQueryOptions = {}) {
  return useQuery<RoleSummary[]>({
    queryKey: ["roles", "select-list"],
    enabled,
    queryFn: async () => {
      const { data, error, response } = await api.client.GET("/roles/select-list");
      if (data) return data;
      throwApiError(error, response, "Failed to load roles.");
    },
    staleTime: 60_000,
  });
}
