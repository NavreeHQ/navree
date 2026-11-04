import { keepPreviousData, useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/vue-query";
import { api, unwrap } from "@/lib/api/client";
import { keys } from "@/lib/api/keys";
import type { ActivityInput, ActivityListQuery, ActivityUpdateInput } from "@/lib/api/types";

/** `filter` (subject_type, subject_id, kind, assignee_id, due, completed), `page`, `per_page`. */
export type ActivityListParams = ActivityListQuery;

export function useActivities(
  params: MaybeRefOrGetter<ActivityListParams>,
  enabled: MaybeRefOrGetter<boolean> = true,
) {
  return useQuery({
    queryKey: computed(() => keys.activities.list(toValue(params))),
    queryFn: () => unwrap(api.GET("/api/v1/activities", { params: { query: toValue(params) } })),
    placeholderData: keepPreviousData,
    enabled: computed(() => toValue(enabled)),
  });
}

function invalidateActivityData(queryClient: QueryClient) {
  void queryClient.invalidateQueries({ queryKey: keys.activities.all });
  void queryClient.invalidateQueries({ queryKey: keys.dashboard });
}

export function useCreateActivity() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: ActivityInput) => unwrap(api.POST("/api/v1/activities", { body })),
    onSuccess: () => invalidateActivityData(queryClient),
  });
}

export function useUpdateActivity() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, ...body }: ActivityUpdateInput & { id: string }) =>
      unwrap(api.PATCH("/api/v1/activities/{id}", { params: { path: { id } }, body })),
    onSuccess: () => invalidateActivityData(queryClient),
  });
}

export function useCompleteActivity() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => unwrap(api.POST("/api/v1/activities/{id}/complete", { params: { path: { id } } })),
    onSuccess: () => invalidateActivityData(queryClient),
  });
}

export function useDeleteActivity() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => unwrap(api.DELETE("/api/v1/activities/{id}", { params: { path: { id } } })),
    onSuccess: () => invalidateActivityData(queryClient),
  });
}
