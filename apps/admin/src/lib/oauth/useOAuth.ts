import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { extractApiErrorMessage } from "../api/errors";

export type OAuthConfigPublicDto =
  components["schemas"]["OAuthConfigPublicDto"];
export type OAuthConfigDto = components["schemas"]["OAuthConfigDto"];
export type UpsertOAuthConfig = components["schemas"]["UpsertOAuthConfig"];
export type DiscoveryRequest = components["schemas"]["DiscoveryRequest"];
export type DiscoveryPrefill = components["schemas"]["DiscoveryPrefill"];
export type ExternalIdentityDto =
  components["schemas"]["ExternalIdentityDto"];

const PUBLIC_KEY = ["oauth", "config", "public"] as const;
const ADMIN_KEY = ["oauth", "config", "admin"] as const;
const IDENTITIES_KEY = ["oauth", "identities"] as const;

export function useOAuthPublicConfig() {
  return useQuery<OAuthConfigPublicDto>({
    queryKey: PUBLIC_KEY,
    queryFn: async () => {
      const { data, error } = await api.client.GET("/auth/oauth/config");
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Failed to load OAuth status."));
    },
    staleTime: 30_000,
  });
}

export function useOAuthAdminConfig() {
  return useQuery<OAuthConfigDto | null>({
    queryKey: ADMIN_KEY,
    queryFn: async () => {
      const { data, error, response } = await api.client.GET(
        "/auth/oauth/admin-config",
      );
      if (data) return data;
      if (response.status === 404) return null;
      throw new Error(extractApiErrorMessage(error, "Failed to load OAuth config."));
    },
    staleTime: 30_000,
  });
}

export function useUpsertOAuthConfig() {
  const qc = useQueryClient();
  return useMutation<OAuthConfigDto, Error, UpsertOAuthConfig>({
    mutationFn: async (input) => {
      const { data, error } = await api.client.POST(
        "/auth/oauth/admin-config",
        { body: input },
      );
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Couldn't save OAuth config."));
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["oauth"] });
    },
  });
}

export function useDeleteOAuthConfig() {
  const qc = useQueryClient();
  return useMutation<void, Error, void>({
    mutationFn: async () => {
      const { error, response } = await api.client.DELETE(
        "/auth/oauth/admin-config",
      );
      if (response.ok) return;
      throw new Error(
        extractApiErrorMessage(error, "Couldn't remove OAuth config."),
      );
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["oauth"] });
    },
  });
}

export function useOAuthDiscover() {
  return useMutation<DiscoveryPrefill, Error, DiscoveryRequest>({
    mutationFn: async (input) => {
      const { data, error } = await api.client.POST(
        "/auth/oauth/discover",
        { body: input },
      );
      if (data) return data;
      throw new Error(
        extractApiErrorMessage(error, "Discovery failed. Check the URL."),
      );
    },
  });
}

export function useMyIdentities() {
  return useQuery<ExternalIdentityDto[]>({
    queryKey: IDENTITIES_KEY,
    queryFn: async () => {
      const { data, error } = await api.client.GET("/auth/oauth/identities");
      if (data) return data;
      throw new Error(
        extractApiErrorMessage(error, "Failed to load linked accounts."),
      );
    },
  });
}

export function useUnlinkIdentity() {
  const qc = useQueryClient();
  return useMutation<void, Error, { id: number }>({
    mutationFn: async ({ id }) => {
      const { error, response } = await api.client.DELETE(
        "/auth/oauth/identities/{id}",
        { params: { path: { id } } },
      );
      if (response.ok) return;
      throw new Error(
        extractApiErrorMessage(error, "Couldn't unlink account."),
      );
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: IDENTITIES_KEY });
    },
  });
}

/**
 * Start the OAuth link flow for the currently signed-in user. The server
 * sets the CSRF state cookie on the response, returns the IdP authorize URL,
 * and the SPA navigates to it (top-level) to trigger the OAuth dance.
 */
export function useStartLink() {
  return useMutation<string, Error, void>({
    mutationFn: async () => {
      // openapi-fetch typing collapses to `never` when requestBody is none.
      // Call the path-specific endpoint directly so we keep the response type.
      const res = await fetch(`${api.baseUrl}/auth/oauth/link/start`, {
        method: "POST",
        headers: {
          Authorization: `Bearer ${localStorage.getItem("open-relay:auth-token:v1") ?? ""}`,
        },
        credentials: "include",
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        throw new Error(
          (body as { error?: string }).error ?? "Couldn't start link flow.",
        );
      }
      const data = (await res.json()) as { authorize_url: string };
      return data.authorize_url;
    },
  });
}
