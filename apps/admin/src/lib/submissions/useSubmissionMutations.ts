import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { components } from "@open-relay/api-client";
import { api } from "../api/client";
import { extractApiErrorMessage } from "../api/errors";

export type RetryDeliveriesResponse =
  components["schemas"]["RetryDeliveriesResponse"];

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
