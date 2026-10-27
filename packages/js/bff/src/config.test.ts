import { describe, expect, it } from "vitest";
import { bffOptionsFromEnv } from "./config.js";

describe("bffOptionsFromEnv", () => {
  it("reads the NAVREE_* settings", () => {
    expect(
      bffOptionsFromEnv({
        NAVREE_API_URL: "http://127.0.0.1:9000",
        NAVREE_COOKIE_SECURE: "0",
        APP_ORIGINS: "https://crm.example.com, https://www.crm.example.com,",
        NAVREE_TRUSTED_PROXY_HOPS: "2",
        NAVREE_WORKSPACE_HEADER: "X-Team",
      }),
    ).toEqual({
      apiUrl: "http://127.0.0.1:9000",
      secureCookies: false,
      allowedOrigins: ["https://crm.example.com", "https://www.crm.example.com"],
      trustedProxyHops: 2,
      workspaceHeader: "X-Team",
    });
  });

  it("has defaults for a local API", () => {
    expect(bffOptionsFromEnv({})).toEqual({ apiUrl: "http://127.0.0.1:8080", trustedProxyHops: 0 });
  });

  it("takes 1/true for the Secure flag and ignores nonsense hop counts", () => {
    expect(bffOptionsFromEnv({ NAVREE_COOKIE_SECURE: "true", NAVREE_TRUSTED_PROXY_HOPS: "many" })).toMatchObject({
      secureCookies: true,
      trustedProxyHops: 0,
    });
    expect(bffOptionsFromEnv({ NAVREE_COOKIE_SECURE: "1" }).secureCookies).toBe(true);
  });

  it("accepts a default API URL and other names (Nuxt's NUXT_ prefix)", () => {
    expect(bffOptionsFromEnv({ NUXT_NAVREE_API_URL: "http://api:8080" }, { apiUrl: "http://127.0.0.1:8092" }).apiUrl).toBe(
      "http://api:8080",
    );
    expect(bffOptionsFromEnv({}, { apiUrl: "http://127.0.0.1:8092" }).apiUrl).toBe("http://127.0.0.1:8092");
  });
});
