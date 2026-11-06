import { describe, expect, it } from "vitest";
import { centsToInput, currencyDigits, dueBucket, formatMoney, parseMoneyToCents } from "./format";

describe("formatMoney", () => {
  it("formats amount_cents with the currency's minor units", () => {
    expect(formatMoney(1_234_567, "INR")).toBe("₹12,345.67");
    expect(formatMoney(99, "INR")).toBe("₹0.99");
    expect(formatMoney(123_456_750, "INR")).toBe("₹12,34,567.50");
    expect(formatMoney(1_000_000_000, "INR")).toBe("₹1,00,00,000");
    expect(formatMoney(1_234_567, "USD")).toBe("$12,345.67");
  });

  it("drops the fraction for whole amounts unless asked", () => {
    expect(formatMoney(4_800_000, "INR")).toBe("₹48,000");
    expect(formatMoney(4_800_000, "INR", { showCents: true })).toBe("₹48,000.00");
  });

  it("formats other currencies", () => {
    expect(formatMoney(1_250_000, "EUR")).toBe("€12,500");
    expect(formatMoney(1_250_050, "GBP")).toBe("£12,500.50");
    expect(formatMoney(0, "INR")).toBe("₹0");
  });

  it("treats zero-decimal currencies' amount_cents as whole units", () => {
    expect(currencyDigits("JPY")).toBe(0);
    expect(formatMoney(5000, "JPY")).toBe("JP¥5,000");
  });

  it("uses compact notation for totals", () => {
    expect(formatMoney(125_000_000, "INR", { compact: true })).toBe("₹12.5L");
    expect(formatMoney(1_000_000_000, "INR", { compact: true })).toBe("₹1Cr");
    expect(formatMoney(4_850_000, "INR", { compact: true })).toBe("₹48.5K");
    expect(formatMoney(50_000, "INR", { compact: true })).toBe("₹500");
  });

  it("accepts lower-case currency codes and defaults to INR", () => {
    expect(formatMoney(1000, "inr")).toBe("₹10");
    expect(formatMoney(1000)).toBe("₹10");
  });
});

describe("parseMoneyToCents", () => {
  it("parses typed amounts into integer minor units", () => {
    expect(parseMoneyToCents("12,500", "INR")).toBe(1_250_000);
    expect(parseMoneyToCents("₹1,200.50", "INR")).toBe(120_050);
    expect(parseMoneyToCents("₹12,34,567.50", "INR")).toBe(123_456_750);
    expect(parseMoneyToCents("0.1", "INR")).toBe(10);
    expect(parseMoneyToCents("5000", "JPY")).toBe(5000);
  });

  it("rejects input that isn't a number", () => {
    expect(parseMoneyToCents("", "INR")).toBeNull();
    expect(parseMoneyToCents("abc", "INR")).toBeNull();
    expect(parseMoneyToCents("1.2.3", "INR")).toBeNull();
  });

  it("round-trips through centsToInput", () => {
    expect(centsToInput(1_250_050, "INR")).toBe("12500.50");
    expect(centsToInput(1_250_000, "INR")).toBe("12500");
    expect(parseMoneyToCents(centsToInput(98_765, "INR"), "INR")).toBe(98_765);
  });
});

describe("dueBucket", () => {
  const now = new Date(Date.UTC(2026, 9, 25, 14, 0, 0));

  it("buckets due dates by UTC day, like the API's due filter", () => {
    expect(dueBucket(new Date(Date.UTC(2026, 9, 24, 23, 59)).toISOString(), now)).toBe("overdue");
    expect(dueBucket(new Date(Date.UTC(2026, 9, 25, 0, 0)).toISOString(), now)).toBe("today");
    expect(dueBucket(new Date(Date.UTC(2026, 9, 25, 9, 0)).toISOString(), now)).toBe("today");
    expect(dueBucket(new Date(Date.UTC(2026, 9, 25, 23, 59)).toISOString(), now)).toBe("today");
    expect(dueBucket(new Date(Date.UTC(2026, 9, 26, 0, 0)).toISOString(), now)).toBe("upcoming");
    expect(dueBucket(null, now)).toBeNull();
  });
});
