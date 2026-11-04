"use client";

import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, unwrap } from "@/lib/api/client";
import { ApiError } from "@/lib/api/errors";
import { keys } from "@/lib/api/keys";
import type { ContactExportQuery, ContactInput, ContactListQuery, ContactUpdateInput } from "@/lib/api/types";

/** `filter` (company_id, status, owner_id), `search`, `page`, `per_page`: sent as `filter[status]=lead`. */
export type ContactListParams = ContactListQuery;

export function useContacts(params: ContactListParams, enabled = true) {
  return useQuery({
    queryKey: keys.contacts.list(params),
    queryFn: () => unwrap(api.GET("/api/v1/contacts", { params: { query: params } })),
    placeholderData: keepPreviousData,
    enabled,
  });
}

export function useContact(id: string) {
  return useQuery({
    queryKey: keys.contacts.detail(id),
    queryFn: () => unwrap(api.GET("/api/v1/contacts/{id}", { params: { path: { id } } })),
  });
}

export function useCreateContact() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: ContactInput) => unwrap(api.POST("/api/v1/contacts", { body })),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: keys.contacts.all });
      void queryClient.invalidateQueries({ queryKey: keys.companies.all });
    },
  });
}

export function useUpdateContact(id: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: ContactUpdateInput) =>
      unwrap(api.PATCH("/api/v1/contacts/{id}", { params: { path: { id } }, body })),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: keys.contacts.all });
      void queryClient.invalidateQueries({ queryKey: keys.companies.all });
      void queryClient.invalidateQueries({ queryKey: keys.deals.all });
    },
  });
}

export function useDeleteContact() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => unwrap(api.DELETE("/api/v1/contacts/{id}", { params: { path: { id } } })),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: keys.contacts.all });
      void queryClient.invalidateQueries({ queryKey: keys.companies.all });
    },
  });
}

/** The path of `GET /api/v1/contacts/export` for a query: `?filter[status]=lead&search=nair`, blank values left out. */
export function contactsExportPath({ filter, search }: ContactExportQuery): string {
  const params = new URLSearchParams();
  for (const [name, value] of Object.entries(filter ?? {})) {
    if (typeof value === "string" && value !== "") params.append(`filter[${name}]`, value);
  }
  if (search) params.set("search", search);
  const query = params.toString();
  return `/api/v1/contacts/export${query ? `?${query}` : ""}`;
}

/** Downloads `GET /api/v1/contacts/export` (text/csv) with the current filters. */
export async function exportContactsCsv(query: ContactExportQuery): Promise<void> {
  const response = await api.rawFetch(contactsExportPath(query), {
    headers: { Accept: "text/csv" },
  });
  if (!response.ok) {
    throw ApiError.fromResponse(response, await response.json().catch(() => null));
  }
  const blob = await response.blob();
  const disposition = response.headers.get("Content-Disposition") ?? "";
  const filename = /filename="?([^";]+)"?/.exec(disposition)?.[1] ?? "contacts.csv";
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.append(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
