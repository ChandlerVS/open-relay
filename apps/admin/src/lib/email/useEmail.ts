import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { extractApiErrorMessage } from "../api/errors";

export type EmailConfigDto = components["schemas"]["EmailConfigDto"];
export type UpsertEmailConfig = components["schemas"]["UpsertEmailConfig"];
export type EmailKindInfo = components["schemas"]["EmailKindInfo"];
export type EmailTestResult = components["schemas"]["EmailTestResult"];

const CONFIG_KEY = ["email", "config"] as const;
const KINDS_KEY = ["email", "kinds"] as const;

/**
 * The active email provider, or `null` when none is configured.
 *
 * `enabled` mirrors the gated-query pattern the storage hooks use: a caller
 * without `email_config:write` shouldn't fire a request that can only 403.
 */
export function useEmailConfig(opts?: { enabled?: boolean }) {
  return useQuery<EmailConfigDto | null>({
    queryKey: CONFIG_KEY,
    enabled: opts?.enabled ?? true,
    queryFn: async () => {
      const { data, error, response } = await api.client.GET("/email");
      if (data) return data;
      // "Not configured" is a normal state, not a failure.
      if (response.status === 404) return null;
      throw new Error(
        extractApiErrorMessage(error, "Failed to load email settings."),
      );
    },
    staleTime: 30_000,
  });
}

export function useEmailKinds(opts?: { enabled?: boolean }) {
  return useQuery<EmailKindInfo[]>({
    queryKey: KINDS_KEY,
    enabled: opts?.enabled ?? true,
    queryFn: async () => {
      const { data, error } = await api.client.GET("/email/kinds");
      if (data) return data;
      throw new Error(
        extractApiErrorMessage(error, "Failed to load email providers."),
      );
    },
    staleTime: 5 * 60_000,
  });
}

export function useUpsertEmailConfig() {
  const qc = useQueryClient();
  return useMutation<EmailConfigDto, Error, UpsertEmailConfig>({
    mutationFn: async (input) => {
      const { data, error } = await api.client.POST("/email", { body: input });
      if (data) return data;
      throw new Error(
        extractApiErrorMessage(error, "Couldn't save the email provider."),
      );
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["email"] });
    },
  });
}

export function useDeleteEmailConfig() {
  const qc = useQueryClient();
  return useMutation<void, Error, void>({
    mutationFn: async () => {
      const { error, response } = await api.client.DELETE("/email");
      if (response.ok) return;
      throw new Error(
        extractApiErrorMessage(error, "Couldn't remove the email provider."),
      );
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["email"] });
    },
  });
}

/**
 * Send a real test message through the saved provider.
 *
 * A rejection (typically an unverified recipient in the SES sandbox) comes
 * back as a *successful* response carrying `ok: false` and a message, so the
 * caller reads `result.ok` rather than treating it as a thrown error.
 */
export function useSendTestEmail() {
  return useMutation<EmailTestResult, Error, string>({
    mutationFn: async (to) => {
      const { data, error } = await api.client.POST("/email/test", {
        body: { to },
      });
      if (data) return data;
      throw new Error(
        extractApiErrorMessage(error, "Couldn't send the test email."),
      );
    },
  });
}
