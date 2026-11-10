// @vitest-environment node
import { describe, expect, it } from "vitest";
import { serveFakeNavree, tokenPair } from "./testing.js";

describe("serveFakeNavree", () => {
  it("answers the scripted routes on a real port and records the calls", async () => {
    const api = await serveFakeNavree({
      "POST /api/auth/login": (call) => Response.json({ received: JSON.parse(call.body), ...tokenPair(1) }),
      "GET /api/v1/deals": (call) => Response.json({ auth: call.headers.get("authorization") }),
    });
    try {
      const login = await fetch(`${api.url}/api/auth/login`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ email: "a" }),
      });
      expect(await login.json()).toMatchObject({ received: { email: "a" }, access_token: "AT1" });

      const deals = await fetch(`${api.url}/api/v1/deals?page=2`, { headers: { authorization: "Bearer AT1" } });
      expect(await deals.json()).toEqual({ auth: "Bearer AT1" });

      const missing = await fetch(`${api.url}/api/nothing`);
      expect(missing.status).toBe(404);
      expect((await missing.json()).code).toBe("not_found");

      expect(api.calls.map((c) => `${c.method} ${new URL(c.url).pathname}${new URL(c.url).search}`)).toEqual([
        "POST /api/auth/login",
        "GET /api/v1/deals?page=2",
        "GET /api/nothing",
      ]);
    } finally {
      await api.close();
    }
  });
});
