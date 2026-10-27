import { keepPreviousData, useQuery } from "@tanstack/vue-query";
import { api, unwrap } from "@/lib/api/client";
import { keys } from "@/lib/api/keys";

export type AuditListParams = {
  actor_id?: string;
  subject_type?: string;
  subject_id?: string;
  action?: string;
  page?: number;
  per_page?: number;
};

export function useAuditLog(params: MaybeRefOrGetter<AuditListParams>, enabled: MaybeRefOrGetter<boolean>) {
  return useQuery({
    queryKey: computed(() => keys.audit.list(toValue(params))),
    queryFn: () => unwrap(api.GET("/api/audit", { params: { query: toValue(params) } })),
    placeholderData: keepPreviousData,
    enabled: computed(() => toValue(enabled)),
    retry: false,
  });
}
