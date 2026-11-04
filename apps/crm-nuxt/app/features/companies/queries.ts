import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/vue-query";
import { api, unwrap } from "@/lib/api/client";
import { keys } from "@/lib/api/keys";
import type { CompanyInput, CompanyListQuery, CompanyUpdateInput } from "@/lib/api/types";

/** `filter` (owner_id, industry), `search`, `sort`, `page`, `per_page`: sent as `filter[industry]=Logistics`. */
export type CompanyListParams = CompanyListQuery;

export function useCompanies(params: MaybeRefOrGetter<CompanyListParams>, enabled: MaybeRefOrGetter<boolean> = true) {
  return useQuery({
    queryKey: computed(() => keys.companies.list(toValue(params))),
    queryFn: () => unwrap(api.GET("/api/v1/companies", { params: { query: toValue(params) } })),
    placeholderData: keepPreviousData,
    enabled: computed(() => toValue(enabled)),
  });
}

export function useCompany(id: MaybeRefOrGetter<string>, enabled: MaybeRefOrGetter<boolean> = true) {
  return useQuery({
    queryKey: computed(() => keys.companies.detail(toValue(id))),
    queryFn: () => unwrap(api.GET("/api/v1/companies/{id}", { params: { path: { id: toValue(id) } } })),
    enabled: computed(() => toValue(enabled) && Boolean(toValue(id))),
  });
}

export function useCreateCompany() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: CompanyInput) => unwrap(api.POST("/api/v1/companies", { body })),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: keys.companies.all }),
  });
}

export function useUpdateCompany(id: MaybeRefOrGetter<string>) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: CompanyUpdateInput) =>
      unwrap(api.PATCH("/api/v1/companies/{id}", { params: { path: { id: toValue(id) } }, body })),
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
