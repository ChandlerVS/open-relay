import { useQuery } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { extractApiErrorMessage } from "../api/errors";

export type PermissionInfo = components["schemas"]["PermissionInfo"];

/**
 * Static catalogue — only changes on deploy. Cached for the life of the
 * session so role-editor mounts find it warm.
 */
export function usePermissionCatalog() {
  return useQuery<PermissionInfo[]>({
    queryKey: ["permissions", "catalog"],
    queryFn: async () => {
      const { data, error } = await api.client.GET("/permissions");
      if (data) return data;
      throw new Error(
        extractApiErrorMessage(error, "Failed to load permissions catalogue."),
      );
    },
    staleTime: Infinity,
    gcTime: Infinity,
  });
}
