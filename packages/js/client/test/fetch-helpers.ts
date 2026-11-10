import { createClient } from "../src/fetch.js";
import type { paths } from "./fixtures/typed.js";

export const BASE = "http://api.test";

export const json = (body: unknown, init: ResponseInit = {}) =>
  new Response(JSON.stringify(body), { headers: { "content-type": "application/json" }, ...init });

/** A client whose fetch records every request and answers with `respond`. */
export function recording(
  respond: (request: Request) => Response | Promise<Response> = () => json({}),
  options: Parameters<typeof createClient>[0] = {},
) {
  const requests: Request[] = [];
  const client = createClient<paths>({
    baseUrl: BASE,
    ...options,
    fetch: async (request) => {
      requests.push(request);
      return respond(request);
    },
  });
  return { client, requests };
}
