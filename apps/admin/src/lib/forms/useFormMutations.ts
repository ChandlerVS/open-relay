import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { extractApiErrorMessage } from "../api/errors";
import type { FormDto } from "./useForms";

export type NewForm = components["schemas"]["NewForm"];
export type UpdateForm = components["schemas"]["UpdateForm"];

function invalidateForms(qc: ReturnType<typeof useQueryClient>) {
  qc.invalidateQueries({ queryKey: ["forms"] });
}

export function useCreateForm() {
  const qc = useQueryClient();
  return useMutation<FormDto, Error, NewForm>({
    mutationFn: async (input) => {
      const { data, error } = await api.client.POST("/forms", { body: input });
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Couldn't create form."));
    },
    onSuccess: () => invalidateForms(qc),
  });
}

export function useUpdateForm() {
  const qc = useQueryClient();
  return useMutation<FormDto, Error, { id: number; input: UpdateForm }>({
    mutationFn: async ({ id, input }) => {
      const { data, error } = await api.client.PATCH("/forms/{id}", {
        params: { path: { id } },
        body: input,
      });
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Couldn't update form."));
    },
    onSuccess: () => invalidateForms(qc),
  });
}

export function useDeleteForm() {
  const qc = useQueryClient();
  return useMutation<void, Error, { id: number }>({
    mutationFn: async ({ id }) => {
      const { error, response } = await api.client.DELETE("/forms/{id}", {
        params: { path: { id } },
      });
      if (response.ok) return;
      throw new Error(extractApiErrorMessage(error, "Couldn't delete form."));
    },
    onSuccess: () => invalidateForms(qc),
  });
}
