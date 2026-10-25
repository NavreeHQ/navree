"use client";

import { keepPreviousData, useQuery } from "@tanstack/react-query";
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

export function useAuditLog(params: AuditListParams, enabled: boolean) {
  return useQuery({
    queryKey: keys.audit.list(params),
    queryFn: () => unwrap(api.GET("/api/audit", { params: { query: params } })),
    placeholderData: keepPreviousData,
    enabled,
    retry: false,
  });
}
