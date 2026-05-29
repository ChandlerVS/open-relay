import { createApiClient } from "@open-relay/api-client";

const baseUrl = import.meta.env.VITE_API_URL ?? "http://localhost:8080";

export const api = {
  baseUrl,
  client: createApiClient({ baseUrl }),
};
