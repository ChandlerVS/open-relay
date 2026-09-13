import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { extractApiErrorMessage } from "../api/errors";

export type ApiKeyDto = components["schemas"]["ApiKeyDto"];
export type NewApiKey = components["schemas"]["NewApiKey"];
export type CreatedApiKey = components["schemas"]["CreatedApiKey"];

const KEYS = ["api-keys"] as const;

export function useApiKeys() {
  return useQuery<ApiKeyDto[]>({
    queryKey: KEYS,
    queryFn: async () => {
      const { data, error } = await api.client.GET("/api-keys");
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Failed to load API keys."));
    },
  });
}

export function useCreateApiKey() {
  const qc = useQueryClient();
  return useMutation<CreatedApiKey, Error, NewApiKey>({
    mutationFn: async (input) => {
      const { data, error } = await api.client.POST("/api-keys", { body: input });
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Couldn't create the key."));
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: KEYS });
    },
  });
}

export function useRevokeApiKey() {
  const qc = useQueryClient();
  return useMutation<void, Error, { id: number }>({
    mutationFn: async ({ id }) => {
      const { error, response } = await api.client.DELETE("/api-keys/{id}", {
        params: { path: { id } },
      });
      if (response.ok) return;
      throw new Error(extractApiErrorMessage(error, "Couldn't revoke the key."));
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: KEYS });
    },
  });
}
