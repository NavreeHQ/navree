import { describe, expect, it, vi } from "vitest";
import { NavreeError } from "./errors";
import { direction, errorKey, isRtl, translateError } from "./i18n";

const navreeError = (statusCode: number, code: string, message = "Detail from the API") =>
  NavreeError.fromResponse(new Response(null, { status: statusCode }), {
    statusCode,
    message,
    error: "Error",
    code,
  });

describe("right to left", () => {
  it("knows the right-to-left languages, with or without a region", () => {
    for (const locale of ["ar", "ar-AE", "he", "fa-IR", "ur", "ps", "yi", "AR", "ar_SA"]) {
      expect(isRtl(locale), locale).toBe(true);
    }
    for (const locale of ["en", "en-US", "hi", "fr-CA", "de", "zh-Hans", ""]) {
      expect(isRtl(locale), locale).toBe(false);
    }
  });

  it("follows the script subtag over the language", () => {
    expect(isRtl("pa-Arab")).toBe(true);
    expect(isRtl("az-Arab-IR")).toBe(true);
    expect(isRtl("ku-Latn")).toBe(false);
    expect(isRtl("uz-Cyrl")).toBe(false);
  });

  it("gives the dir attribute value", () => {
    expect(direction("ar")).toBe("rtl");
    expect(direction("en")).toBe("ltr");
    expect(direction(undefined)).toBe("ltr");
  });
});

describe("error-code translation", () => {
  it("names the message key after the code", () => {
    expect(errorKey(navreeError(404, "not_found"))).toBe("errors.not_found");
    expect(errorKey(navreeError(409, "invoice_already_sent"), "api.errors")).toBe("api.errors.invoice_already_sent");
    expect(errorKey(new Error("x"))).toBe("errors.unknown");
    expect(errorKey(new TypeError("Failed to fetch"))).toBe("errors.network_error");
  });

  it("translates with next-intl's t (and its t.has)", () => {
    const messages: Record<string, string> = {
      "errors.limit_reached": "لقد بلغت حد خطتك.",
      "errors.unknown": "حدث خطأ ما.",
    };
    const t = Object.assign(
      vi.fn((key: string) => messages[key] ?? key),
      { has: (key: string) => key in messages },
    );

    expect(translateError(navreeError(409, "limit_reached"), t)).toBe("لقد بلغت حد خطتك.");
    expect(t).toHaveBeenCalledWith("errors.limit_reached", { status: 409, message: "Detail from the API" });
    // No message for this code: the catalogue's `unknown`.
    expect(translateError(navreeError(409, "invoice_already_sent"), t)).toBe("حدث خطأ ما.");
  });

  it("translates with vue-i18n's t and te", () => {
    const messages: Record<string, string> = { "errors.not_found": "Introuvable." };
    const t = (key: string) => messages[key] ?? key;
    const te = (key: string) => key in messages;

    expect(translateError(navreeError(404, "not_found"), t, { has: te })).toBe("Introuvable.");
  });

  it("falls back to the English sentence when the catalogue has neither the code nor `unknown`", () => {
    const t = Object.assign((key: string) => key, { has: () => false });
    expect(translateError(navreeError(403, "demo_read_only", "Demo workspaces are read-only."), t)).toBe(
      "Demo workspaces are read-only.",
    );
    expect(translateError("boom", t, { fallback: "Could not save." })).toBe("Could not save.");
  });

  it("uses the key as is when the translator can't say which keys exist", () => {
    const t = (key: string) => `[${key}]`;
    expect(translateError(navreeError(429, "too_many_requests"), t)).toBe("[errors.too_many_requests]");
  });
});
