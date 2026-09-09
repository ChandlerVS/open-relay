import { useQuery } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { throwApiError } from "../api/errors";
import type { GatedQueryOptions } from "../api/queryClient";

export type FormDto = components["schemas"]["FormDto"];
export type FormList = components["schemas"]["FormList"];
export type FormSelectOption = components["schemas"]["FormSelectOption"];
export type EmbedSnippet = components["schemas"]["EmbedSnippetDto"];

export interface FormsListParams {
  limit?: number;
  offset?: number;
}

export function useFormsList(params: FormsListParams = {}) {
  const { limit, offset } = params;
  return useQuery<FormList>({
    queryKey: ["forms", "list", { limit, offset }],
    queryFn: async () => {
      const query: Record<string, number> = {};
      if (typeof limit === "number") query.limit = limit;
      if (typeof offset === "number") query.offset = offset;
      const { data, error, response } = await api.client.GET("/forms", {
        params: { query },
      });
      if (data) return data;
      throwApiError(error, response, "Failed to load forms.");
    },
    staleTime: 30_000,
  });
}

export function useForm(id: number | null) {
  return useQuery<FormDto>({
    queryKey: ["forms", "detail", id],
    enabled: id != null,
    queryFn: async () => {
      const { data, error, response } = await api.client.GET("/forms/{id}", {
        params: { path: { id: id as number } },
      });
      if (data) return data;
      throwApiError(error, response, "Failed to load form.");
    },
  });
}

export function useFormEmbed(id: number | null) {
  return useQuery<EmbedSnippet>({
    queryKey: ["forms", "embed", id],
    enabled: id != null,
    queryFn: async () => {
      const { data, error, response } = await api.client.GET("/forms/{id}/embed", {
        params: { path: { id: id as number } },
      });
      if (data) return data;
      throwApiError(error, response, "Failed to load embed code.");
    },
    staleTime: 60_000,
  });
}

export function useFormSelectList({ enabled = true }: GatedQueryOptions = {}) {
  return useQuery<FormSelectOption[]>({
    queryKey: ["forms", "select-list"],
    enabled,
    queryFn: async () => {
      const { data, error, response } = await api.client.GET("/forms/select-list");
      if (data) return data;
      throwApiError(error, response, "Failed to load forms.");
    },
    staleTime: 60_000,
  });
}
