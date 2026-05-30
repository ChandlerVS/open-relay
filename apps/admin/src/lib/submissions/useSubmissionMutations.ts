import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api } from "../api/client";
import { extractApiErrorMessage } from "../api/errors";

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
