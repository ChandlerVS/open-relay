import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { extractApiErrorMessage } from "../api/errors";
import type { ExportApiQuery } from "./filters";

export type RetryDeliveriesResponse =
  components["schemas"]["RetryDeliveriesResponse"];
export type BulkDeleteResponse = components["schemas"]["BulkDeleteResponse"];

function invalidate(qc: ReturnType<typeof useQueryClient>) {
  qc.invalidateQueries({ queryKey: ["submissions"] });
}

export function useDeleteSubmission() {
  const qc = useQueryClient();
  return useMutation<void, Error, { id: number }>({
    mutationFn: async ({ id }) => {
      const { error, response } = await api.client.DELETE("/submissions/{id}", {
        params: { path: { id } },
      });
      if (response.ok) return;
      throw new Error(extractApiErrorMessage(error, "Couldn't delete submission."));
    },
    onSuccess: () => invalidate(qc),
  });
}

/// Manually re-queue the given delivery rows for another attempt. The worker
/// picks them up on its next poll; the list query is invalidated so the chips
/// reflect the new `pending` state.
export function useRetryDeliveries() {
  const qc = useQueryClient();
  return useMutation<RetryDeliveriesResponse, Error, { deliveryIds: number[] }>({
    mutationFn: async ({ deliveryIds }) => {
      const { data, error } = await api.client.POST(
        "/submissions/deliveries/retry",
        { body: { delivery_ids: deliveryIds } },
      );
      if (data) return data;
      throw new Error(
        extractApiErrorMessage(error, "Couldn't re-sync deliveries."),
      );
    },
    onSuccess: () => invalidate(qc),
  });
}

export function useBulkDeleteSubmissions() {
  const qc = useQueryClient();
  return useMutation<BulkDeleteResponse, Error, { ids: number[] }>({
    mutationFn: async ({ ids }) => {
      const { data, error } = await api.client.POST("/submissions/bulk-delete", {
        body: { ids },
      });
      if (data) return data;
      throw new Error(extractApiErrorMessage(error, "Couldn't delete submissions."));
    },
    onSuccess: () => invalidate(qc),
  });
}

/**
 * Download every submission matching `query` as CSV. Fetched through the API
 * client rather than a plain link so the bearer token and the 401 refresh
 * apply; the blob is then handed to the browser as a download.
 *
 * The file name is built here rather than read off `Content-Disposition`: the
 * admin runs on another origin in development, and that header isn't
 * CORS-readable without an `expose_headers` the server doesn't set.
 */
export function useExportSubmissions() {
  return useMutation<void, Error, { query: ExportApiQuery }>({
    mutationFn: async ({ query }) => {
      const { data, error, response } = await api.client.GET("/submissions/export", {
        params: { query },
        parseAs: "blob",
      });
      if (!response.ok || !data) {
        throw new Error(extractApiErrorMessage(error, "Couldn't export submissions."));
      }
      const d = new Date();
      const stamp = `${d.getFullYear()}${String(d.getMonth() + 1).padStart(2, "0")}${String(d.getDate()).padStart(2, "0")}`;
      downloadBlob(data, `submissions-${stamp}.csv`);
    },
  });
}

function downloadBlob(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  // Revoking synchronously can cancel the download in some browsers.
  setTimeout(() => URL.revokeObjectURL(url), 1_000);
}
