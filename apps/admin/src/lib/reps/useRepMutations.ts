import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { extractApiErrorMessage } from "../api/errors";
import type { RepDto } from "./useReps";

export type NewRep = components["schemas"]["NewRep"];
export type UpdateRep = components["schemas"]["UpdateRep"];

function invalidateReps(qc: ReturnType<typeof useQueryClient>) {
  qc.invalidateQueries({ queryKey: ["reps"] });
}

export function useCreateRep() {
  const qc = useQueryClient();
  return useMutation<RepDto, Error, NewRep>({
    mutationFn: async (input) => {
      const { data, error } = await api.client.POST("/reps", { body: input });
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Couldn't create rep."));
    },
    onSuccess: () => invalidateReps(qc),
  });
}

export function useUpdateRep() {
  const qc = useQueryClient();
  return useMutation<RepDto, Error, { id: number; input: UpdateRep }>({
    mutationFn: async ({ id, input }) => {
      const { data, error } = await api.client.PATCH("/reps/{id}", {
        params: { path: { id } },
        body: input,
      });
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Couldn't update rep."));
    },
    onSuccess: () => invalidateReps(qc),
  });
}

export function useDeleteRep() {
  const qc = useQueryClient();
  return useMutation<void, Error, { id: number }>({
    mutationFn: async ({ id }) => {
      const { error, response } = await api.client.DELETE("/reps/{id}", {
        params: { path: { id } },
      });
      if (response.ok) return;
      throw new Error(extractApiErrorMessage(error, "Couldn't delete rep."));
    },
    onSuccess: () => invalidateReps(qc),
  });
}
