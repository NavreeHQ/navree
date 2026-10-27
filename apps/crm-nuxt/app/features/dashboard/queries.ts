import { useQuery } from "@tanstack/vue-query";
import { api, unwrap } from "@/lib/api/client";
import { keys } from "@/lib/api/keys";

export function useDashboard() {
  return useQuery({
    queryKey: keys.dashboard,
    queryFn: () => unwrap(api.GET("/api/v1/dashboard")),
  });
}
