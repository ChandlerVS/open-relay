import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { extractApiErrorMessage } from "../api/errors";

export type StorageConfigDto = components["schemas"]["StorageConfigDto"];
export type UpsertStorageConfig = components["schemas"]["UpsertStorageConfig"];
export type StorageKindInfo = components["schemas"]["StorageKindInfo"];
export type StorageTestResult = components["schemas"]["StorageTestResult"];

const CONFIG_KEY = ["storage", "config"] as const;
const KINDS_KEY = ["storage", "kinds"] as const;

/**
 * The active storage provider, or `null` when none is configured.
 *
 * `enabled` mirrors the gated-query pattern the backends hooks use: a caller
 * without `storage_config:write` (the builder's "no provider" warning asks
 * this question too) shouldn't fire a request that can only 403.
 */
export function useStorageConfig(opts?: { enabled?: boolean }) {
  return useQuery<StorageConfigDto | null>({
    queryKey: CONFIG_KEY,
    enabled: opts?.enabled ?? true,
    queryFn: async () => {
      const { data, error, response } = await api.client.GET("/storage");
      if (data) return data;
      // "Not configured" is a normal state, not a failure.
      if (response.status === 404) return null;
      throw new Error(
        extractApiErrorMessage(error, "Failed to load storage settings."),
      );
    },
    staleTime: 30_000,
  });
}

export function useStorageKinds(opts?: { enabled?: boolean }) {
  return useQuery<StorageKindInfo[]>({
    queryKey: KINDS_KEY,
    enabled: opts?.enabled ?? true,
    queryFn: async () => {
      const { data, error } = await api.client.GET("/storage/kinds");
      if (data) return data;
      throw new Error(
        extractApiErrorMessage(error, "Failed to load storage kinds."),
      );
    },
    staleTime: 5 * 60_000,
  });
}

export function useUpsertStorageConfig() {
  const qc = useQueryClient();
  return useMutation<StorageConfigDto, Error, UpsertStorageConfig>({
    mutationFn: async (input) => {
      const { data, error } = await api.client.POST("/storage", { body: input });
      if (data) return data;
      throw new Error(
        extractApiErrorMessage(error, "Couldn't save the storage provider."),
      );
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["storage"] });
    },
  });
}

export function useDeleteStorageConfig() {
  const qc = useQueryClient();
  return useMutation<void, Error, void>({
    mutationFn: async () => {
      const { error, response } = await api.client.DELETE("/storage");
      if (response.ok) return;
      throw new Error(
        extractApiErrorMessage(error, "Couldn't remove the storage provider."),
      );
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["storage"] });
    },
  });
}

/**
 * Probe the saved credentials.
 *
 * Note the result shape: a reachable-but-rejecting store comes back as a
 * *successful* response carrying `ok: false` and a message, so the caller
 * reads `result.ok` rather than treating a bad bucket as a thrown error.
 */
export function useTestStorageConfig() {
  return useMutation<StorageTestResult, Error, void>({
    mutationFn: async () => {
      const { data, error } = await api.client.POST("/storage/test", {});
      if (data) return data;
      throw new Error(
        extractApiErrorMessage(error, "Couldn't reach the storage provider."),
      );
    },
  });
}
