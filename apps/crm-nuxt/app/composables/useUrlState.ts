import type { LocationQueryRaw } from "vue-router";

type Value = string | number | boolean | null | undefined;

/**
 * List filters live in the URL so views can be shared and survive reloads.
 * `set({ search: "agra" })` replaces the history entry and, by default, resets
 * pagination. A late debounced update is dropped once the person has left the
 * page, so it can never drag them back.
 */
export function useUrlState() {
  const route = useRoute();
  const router = useRouter();
  const path = route.path;

  function get(key: string): string | undefined {
    const value = route.query[key];
    const first = Array.isArray(value) ? value[0] : value;
    return typeof first === "string" && first !== "" ? first : undefined;
  }

  function set(updates: Record<string, Value>, { resetPage = true }: { resetPage?: boolean } = {}): void {
    if (route.path !== path) return;
    const next: LocationQueryRaw = { ...route.query };
    for (const [key, value] of Object.entries(updates)) {
      if (value === undefined || value === null || value === "" || value === false) delete next[key];
      else next[key] = String(value);
    }
    if (resetPage && !("page" in updates)) delete next.page;
    void router.replace({ path, query: next });
  }

  const page = computed(() => Math.max(1, Number(get("page")) || 1));

  return { get, set, page, query: computed(() => route.query) };
}
