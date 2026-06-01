import { useQuery } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { extractApiErrorMessage } from "../api/errors";

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
      const { data, error } = await api.client.GET("/forms", {
        params: { query },
      });
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Failed to load forms."));
    },
    staleTime: 30_000,
  });
}

export function useForm(id: number | null) {
  return useQuery<FormDto>({
    queryKey: ["forms", "detail", id],
    enabled: id != null,
    queryFn: async () => {
      const { data, error } = await api.client.GET("/forms/{id}", {
        params: { path: { id: id as number } },
      });
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Failed to load form."));
    },
  });
}

export function useFormEmbed(id: number | null) {
  return useQuery<EmbedSnippet>({
    queryKey: ["forms", "embed", id],
    enabled: id != null,
    queryFn: async () => {
      const { data, error } = await api.client.GET("/forms/{id}/embed", {
        params: { path: { id: id as number } },
      });
      if (data) return data;
      throw new Error(
        extractApiErrorMessage(error, "Failed to load embed code."),
      );
    },
    staleTime: 60_000,
  });
}

export function useFormSelectList() {
  return useQuery<FormSelectOption[]>({
    queryKey: ["forms", "select-list"],
    queryFn: async () => {
      const { data, error } = await api.client.GET("/forms/select-list");
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Failed to load forms."));
    },
    staleTime: 60_000,
  });
}
