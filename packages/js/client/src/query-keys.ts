/**
 * Query keys for TanStack Query (React or Vue), one family per resource:
 *
 * ```ts
 * const deals = queryKeys("deals");
 * useQuery({ queryKey: deals.list(query), queryFn: () => unwrap(api.GET("/api/v1/deals", { params: { query } })) });
 * queryClient.invalidateQueries({ queryKey: deals.all }); // every list and record
 * ```
 */

type Extra = Record<string, (...args: never[]) => readonly unknown[]>;

type Prefixed<R extends string, E extends Extra> = {
  [K in keyof E]: (...args: Parameters<E[K]>) => readonly [R, ...ReturnType<E[K]>];
};

export interface ResourceKeys<R extends string> {
  /** Every key of the resource. */
  readonly all: readonly [R];
  /** Every list of the resource. */
  lists(): readonly [R, "list"];
  /** One list: the list query (or any params object) is part of the key. */
  list(query?: object): readonly [R, "list", object];
  /** Every single record of the resource. */
  details(): readonly [R, "detail"];
  /** One record. */
  detail(id: string | number): readonly [R, "detail", string];
}

/** The key family of `resource`, plus `extra` keys nested under it. */
export function queryKeys<const R extends string, const E extends Extra = Record<never, never>>(
  resource: R,
  extra?: E,
): ResourceKeys<R> & Prefixed<R, E> {
  const base: ResourceKeys<R> = {
    all: [resource] as const,
    lists: () => [resource, "list"] as const,
    list: (query?: object) => [resource, "list", query ?? {}] as const,
    details: () => [resource, "detail"] as const,
    detail: (id: string | number) => [resource, "detail", String(id)] as const,
  };
  const prefixed: Record<string, unknown> = {};
  for (const [name, make] of Object.entries(extra ?? {})) {
    prefixed[name] = (...args: never[]) => [resource, ...make(...args)] as const;
  }
  return Object.assign(base, prefixed) as ResourceKeys<R> & Prefixed<R, E>;
}
