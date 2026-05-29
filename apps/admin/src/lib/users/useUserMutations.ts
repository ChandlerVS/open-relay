import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { extractApiErrorMessage } from "../api/errors";
import type { UserDto } from "./useUsers";

export type NewUser = components["schemas"]["NewUser"];
export type UpdateUser = components["schemas"]["UpdateUser"];
export type ChangePassword = components["schemas"]["ChangePassword"];

function invalidateUsers(qc: ReturnType<typeof useQueryClient>) {
  qc.invalidateQueries({ queryKey: ["users"] });
}

export function useCreateUser() {
  const qc = useQueryClient();
  return useMutation<UserDto, Error, NewUser>({
    mutationFn: async (input) => {
      const { data, error } = await api.client.POST("/users", { body: input });
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Couldn't create user."));
    },
    onSuccess: () => invalidateUsers(qc),
  });
}

export function useUpdateUser() {
  const qc = useQueryClient();
  return useMutation<UserDto, Error, { id: number; input: UpdateUser }>({
    mutationFn: async ({ id, input }) => {
      const { data, error } = await api.client.PATCH("/users/{id}", {
        params: { path: { id } },
        body: input,
      });
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Couldn't update user."));
    },
    onSuccess: () => invalidateUsers(qc),
  });
}

export function useChangeUserPassword() {
  return useMutation<void, Error, { id: number; input: ChangePassword }>({
    mutationFn: async ({ id, input }) => {
      const { error, response } = await api.client.POST("/users/{id}/password", {
        params: { path: { id } },
        body: input,
      });
      if (response.ok) return;
      throw new Error(extractApiErrorMessage(error, "Couldn't change password."));
    },
  });
}

export function useDeleteUser() {
  const qc = useQueryClient();
  return useMutation<void, Error, { id: number }>({
    mutationFn: async ({ id }) => {
      const { error, response } = await api.client.DELETE("/users/{id}", {
        params: { path: { id } },
      });
      if (response.ok) return;
      throw new Error(extractApiErrorMessage(error, "Couldn't delete user."));
    },
    onSuccess: () => invalidateUsers(qc),
  });
}
