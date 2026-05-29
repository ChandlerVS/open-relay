import { createApiClient } from "@open-relay/api-client";
import { emitSessionExpired, getCurrentToken } from "./tokenSource";

const baseUrl = import.meta.env.VITE_API_URL ?? "http://localhost:8080";

const client = createApiClient({ baseUrl });

client.use({
  onRequest({ request }) {
    const token = getCurrentToken();
    if (token) request.headers.set("Authorization", `Bearer ${token}`);
  },
  onResponse({ response }) {
    if (response.status === 401 && getCurrentToken()) {
      emitSessionExpired();
    }
  },
});

export const api = {
  baseUrl,
  client,
};

export type Api = typeof api;
