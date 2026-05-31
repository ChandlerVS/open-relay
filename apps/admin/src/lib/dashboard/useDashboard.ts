import { useQuery } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { extractApiErrorMessage } from "../api/errors";

export type DashboardOverview = components["schemas"]["DashboardOverview"];

export function useDashboardOverview() {
  return useQuery<DashboardOverview>({
    queryKey: ["dashboard", "overview"],
    queryFn: async () => {
      const { data, error } = await api.client.GET("/dashboard");
      if (data) return data;
      throw new Error(
        extractApiErrorMessage(error, "Failed to load dashboard."),
      );
    },
    staleTime: 30_000,
  });
}
