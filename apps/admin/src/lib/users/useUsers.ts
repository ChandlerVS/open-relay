import { useQuery } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { extractApiErrorMessage } from "../api/errors";

export type UserDto = components["schemas"]["UserDto"];
export type UserList = components["schemas"]["UserList"];
export type UserSelectOption = components["schemas"]["UserSelectOption"];

export interface UsersListParams {
  limit?: number;
  offset?: number;
}

export function useUsersList(params: UsersListParams = {}) {
  const { limit, offset } = params;
  return useQuery<UserList>({
    queryKey: ["users", "list", { limit, offset }],
    queryFn: async () => {
      const query: Record<string, number> = {};
      if (typeof limit === "number") query.limit = limit;
      if (typeof offset === "number") query.offset = offset;
      const { data, error } = await api.client.GET("/users", {
        params: { query },
      });
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Failed to load users."));
    },
    staleTime: 30_000,
  });
}

export function useUser(id: number | null) {
  return useQuery<UserDto>({
    queryKey: ["users", "detail", id],
    enabled: id != null,
    queryFn: async () => {
      const { data, error } = await api.client.GET("/users/{id}", {
        params: { path: { id: id as number } },
      });
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Failed to load user."));
    },
  });
}

export function useUserSelectList() {
  return useQuery<UserSelectOption[]>({
    queryKey: ["users", "select-list"],
    queryFn: async () => {
      const { data, error } = await api.client.GET("/users/select-list");
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Failed to load users."));
    },
    staleTime: 60_000,
  });
}
