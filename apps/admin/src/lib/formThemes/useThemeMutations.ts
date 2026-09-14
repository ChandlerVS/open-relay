import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { extractApiErrorMessage } from "../api/errors";
import type { ThemeDto } from "./useThemes";

export type NewTheme = components["schemas"]["NewTheme"];
export type UpdateTheme = components["schemas"]["UpdateTheme"];

/**
 * A theme edit changes what forms look like, and a delete rewrites which
 * theme forms point at — so every mutation also invalidates `["forms"]`,
 * which covers the builder's resolved-theme query.
 */
function invalidate(qc: ReturnType<typeof useQueryClient>) {
  qc.invalidateQueries({ queryKey: ["themes"] });
  qc.invalidateQueries({ queryKey: ["forms"] });
}

export function useCreateTheme() {
  const qc = useQueryClient();
  return useMutation<ThemeDto, Error, NewTheme>({
    mutationFn: async (input) => {
      const { data, error } = await api.client.POST("/themes", { body: input });
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Couldn't create theme."));
    },
    onSuccess: () => invalidate(qc),
  });
}

export function useUpdateTheme() {
  const qc = useQueryClient();
  return useMutation<ThemeDto, Error, { id: number; input: UpdateTheme }>({
    mutationFn: async ({ id, input }) => {
      const { data, error } = await api.client.PATCH("/themes/{id}", {
        params: { path: { id } },
        body: input,
      });
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Couldn't update theme."));
    },
    onSuccess: () => invalidate(qc),
  });
}

export function useDeleteTheme() {
  const qc = useQueryClient();
  return useMutation<void, Error, { id: number }>({
    mutationFn: async ({ id }) => {
      const { error, response } = await api.client.DELETE("/themes/{id}", {
        params: { path: { id } },
      });
      if (response.ok) return;
      throw new Error(extractApiErrorMessage(error, "Couldn't delete theme."));
    },
    onSuccess: () => invalidate(qc),
  });
}
