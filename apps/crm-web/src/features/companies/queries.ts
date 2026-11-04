"use client";

import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, unwrap } from "@/lib/api/client";
import { keys } from "@/lib/api/keys";
import type { CompanyInput, CompanyListQuery, CompanyUpdateInput } from "@/lib/api/types";

/** `filter` (owner_id, industry), `search`, `sort`, `page`, `per_page`: sent as `filter[industry]=Logistics`. */
export type CompanyListParams = CompanyListQuery;

export function useCompanies(params: CompanyListParams, enabled = true) {
  return useQuery({
    queryKey: keys.companies.list(params),
    queryFn: () => unwrap(api.GET("/api/v1/companies", { params: { query: params } })),
    placeholderData: keepPreviousData,
    enabled,
  });
}

export function useCompany(id: string) {
  return useQuery({
    queryKey: keys.companies.detail(id),
    queryFn: () => unwrap(api.GET("/api/v1/companies/{id}", { params: { path: { id } } })),
  });
}

export function useCreateCompany() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: CompanyInput) => unwrap(api.POST("/api/v1/companies", { body })),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: keys.companies.all }),
  });
}

export function useUpdateCompany(id: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: CompanyUpdateInput) =>
      unwrap(api.PATCH("/api/v1/companies/{id}", { params: { path: { id } }, body })),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: keys.companies.all });
      // Company names are embedded in contacts, deals and activity labels.
      void queryClient.invalidateQueries({ queryKey: keys.contacts.all });
      void queryClient.invalidateQueries({ queryKey: keys.deals.all });
      void queryClient.invalidateQueries({ queryKey: keys.activities.all });
    },
  });
}

export function useDeleteCompany() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => unwrap(api.DELETE("/api/v1/companies/{id}", { params: { path: { id } } })),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: keys.companies.all }),
  });
}
