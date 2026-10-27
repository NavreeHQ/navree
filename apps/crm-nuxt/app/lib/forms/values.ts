/** Converts "" to null for optional API fields. */
export function nullable(value: string | undefined | null): string | null {
  const v = (value ?? "").trim();
  return v === "" ? null : v;
}
