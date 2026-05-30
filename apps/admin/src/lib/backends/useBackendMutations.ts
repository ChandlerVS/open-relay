import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { extractApiErrorMessage } from "../api/errors";
import type { BackendInstanceDto } from "./useBackends";

export type NewBackendInstance = components["schemas"]["NewBackendInstance"];
export type UpdateBackendInstance = components["schemas"]["UpdateBackendInstance"];
export type BackendInstanceFormRef = components["schemas"]["BackendInstanceFormRef"];

/**
 * Carries the structured 409 payload returned by `DELETE /backends/{id}` when
 * forms still reference the instance. Lets the UI render the blocking list
 * instead of just stringifying the error.
 */
export class BackendInUseError extends Error {
  constructor(public forms: BackendInstanceFormRef[]) {
    super("Backend is still referenced by other forms.");
    this.name = "BackendInUseError";
  }
}

function invalidateBackends(qc: ReturnType<typeof useQueryClient>) {
  qc.invalidateQueries({ queryKey: ["backends"] });
}

export function useCreateBackend() {
  const qc = useQueryClient();
  return useMutation<BackendInstanceDto, Error, NewBackendInstance>({
    mutationFn: async (input) => {
      const { data, error } = await api.client.POST("/backends", { body: input });
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Couldn't create backend."));
    },
    onSuccess: () => invalidateBackends(qc),
  });
}

export function useUpdateBackend() {
  const qc = useQueryClient();
  return useMutation<
    BackendInstanceDto,
    Error,
    { id: number; input: UpdateBackendInstance }
  >({
    mutationFn: async ({ id, input }) => {
      const { data, error } = await api.client.PATCH("/backends/{id}", {
        params: { path: { id } },
        body: input,
      });
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Couldn't update backend."));
    },
    onSuccess: () => invalidateBackends(qc),
  });
}

export function useDeleteBackend() {
  const qc = useQueryClient();
  return useMutation<void, Error, { id: number }>({
    mutationFn: async ({ id }) => {
      const { error, response } = await api.client.DELETE("/backends/{id}", {
        params: { path: { id } },
      });
      if (response.ok) return;
      // 409 carries `{ forms: [...] }`. Other errors keep the existing
      // string-message pathway.
      if (response.status === 409 && error && typeof error === "object" && "forms" in error) {
        const forms = (error as { forms: BackendInstanceFormRef[] }).forms ?? [];
        throw new BackendInUseError(forms);
      }
      throw new Error(extractApiErrorMessage(error, "Couldn't delete backend."));
    },
    onSuccess: () => invalidateBackends(qc),
  });
}
