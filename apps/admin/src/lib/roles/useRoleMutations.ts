import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { extractApiErrorMessage } from "../api/errors";
import type { RoleDto } from "./useRoles";

export type NewRole = components["schemas"]["NewRole"];
export type UpdateRole = components["schemas"]["UpdateRole"];

function invalidateRoles(qc: ReturnType<typeof useQueryClient>) {
  qc.invalidateQueries({ queryKey: ["roles"] });
  // Role membership affects who can see what, so also nudge the session.
  qc.invalidateQueries({ queryKey: ["users"] });
}

export function useCreateRole() {
  const qc = useQueryClient();
  return useMutation<RoleDto, Error, NewRole>({
    mutationFn: async (input) => {
      const { data, error } = await api.client.POST("/roles", { body: input });
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Couldn't create role."));
    },
    onSuccess: () => invalidateRoles(qc),
  });
}

export function useUpdateRole() {
  const qc = useQueryClient();
  return useMutation<RoleDto, Error, { id: number; input: UpdateRole }>({
    mutationFn: async ({ id, input }) => {
      const { data, error } = await api.client.PATCH("/roles/{id}", {
        params: { path: { id } },
        body: input,
      });
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Couldn't update role."));
    },
    onSuccess: () => invalidateRoles(qc),
  });
}

export function useDeleteRole() {
  const qc = useQueryClient();
  return useMutation<void, Error, { id: number }>({
    mutationFn: async ({ id }) => {
      const { error, response } = await api.client.DELETE("/roles/{id}", {
        params: { path: { id } },
      });
      if (response.ok) return;
      throw new Error(extractApiErrorMessage(error, "Couldn't delete role."));
    },
    onSuccess: () => invalidateRoles(qc),
  });
}
