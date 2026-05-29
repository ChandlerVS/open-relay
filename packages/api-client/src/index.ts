import createClient, { type Client, type ClientOptions } from "openapi-fetch";
import type { paths } from "./generated/schema";

export type ApiClient = Client<paths>;

export function createApiClient(opts: ClientOptions): ApiClient {
  return createClient<paths>(opts);
}

export type { paths, components, operations } from "./generated/schema";
