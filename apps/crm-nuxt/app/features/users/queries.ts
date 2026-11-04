import { useMutation, useQuery, useQueryClient } from "@tanstack/vue-query";
import { api, unwrap } from "@/lib/api/client";
import { keys } from "@/lib/api/keys";
import type { UserCreateInput, UserListQuery, UserUpdateInput } from "@/lib/api/types";

export type UserListParams = UserListQuery;

/** The whole (small) team for owner pickers and name lookups, or a filtered page. */
export function useUsers(params: MaybeRefOrGetter<UserListParams> = { per_page: 100 }) {
  return useQuery({
    queryKey: computed(() => keys.users.list(toValue(params))),
    queryFn: () => unwrap(api.GET("/api/v1/users", { params: { query: toValue(params) } })),
    staleTime: 60_000,
  });
}

export function useCreateUser() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: UserCreateInput) => unwrap(api.POST("/api/v1/users", { body })),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: keys.users.all });
      void queryClient.invalidateQueries({ queryKey: keys.audit.all });
    },
  });
}

export function useUpdateUser() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, ...body }: UserUpdateInput & { id: string }) =>
      unwrap(api.PATCH("/api/v1/users/{id}", { params: { path: { id } }, body })),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: keys.users.all });
      void queryClient.invalidateQueries({ queryKey: keys.audit.all });
    },
  });
}
