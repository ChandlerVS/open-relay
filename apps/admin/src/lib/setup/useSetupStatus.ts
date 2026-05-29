import { useQuery } from "@tanstack/react-query";
import { api } from "../api/client";

export function useSetupStatus() {
  return useQuery({
    queryKey: ["setup", "status"],
    queryFn: async () => {
      const { data, error } = await api.client.GET("/setup/status");
      if (error || !data) throw new Error("failed to load setup status");
      return data;
    },
    staleTime: 5 * 60_000,
    refetchOnReconnect: false,
  });
}
