import { useQuery } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { throwApiError } from "../api/errors";
import type { GatedQueryOptions } from "../api/queryClient";

// "Form themes" rather than "themes": `lib/theme` is the admin's own
// light/dark mode, and the two must not be confused at an import site.
export type ThemeDto = components["schemas"]["ThemeDto"];
export type ThemeList = components["schemas"]["ThemeList"];
export type ThemeSettings = components["schemas"]["ThemeSettings"];
export type ThemeColors = components["schemas"]["ThemeColors"];

export function useThemesList({ enabled = true }: GatedQueryOptions = {}) {
  return useQuery<ThemeList>({
    queryKey: ["themes", "list"],
    enabled,
    queryFn: async () => {
      const { data, error, response } = await api.client.GET("/themes");
      if (data) return data;
      throwApiError(error, response, "Failed to load themes.");
    },
    staleTime: 30_000,
  });
}

export function useFormTheme(id: number | null) {
  return useQuery<ThemeDto>({
    queryKey: ["themes", "detail", id],
    enabled: id != null,
    queryFn: async () => {
      const { data, error, response } = await api.client.GET("/themes/{id}", {
        params: { path: { id: id as number } },
      });
      if (data) return data;
      throwApiError(error, response, "Failed to load theme.");
    },
  });
}

/**
 * The theme a form actually renders with, read off the public endpoint rather
 * than resolved here. "Own theme, else the default, else built-in" lives in
 * `themes::service::resolve_settings` on the server; repeating it client-side
 * would be a second copy of the fallback, and would need `themes:read` that a
 * forms-only editor may not hold.
 */
export function useResolvedFormTheme(formId: number | null) {
  return useQuery({
    queryKey: ["forms", "public", formId],
    enabled: formId != null,
    queryFn: async () => {
      const { data, error, response } = await api.client.GET("/public/forms/{id}", {
        params: { path: { id: formId as number } },
      });
      if (data) return data;
      throwApiError(error, response, "Failed to load form theme.");
    },
    select: (dto) => dto.theme ?? null,
    staleTime: 30_000,
  });
}
