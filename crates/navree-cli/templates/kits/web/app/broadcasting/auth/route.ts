import { bff } from "@/lib/bff/server";

/** Private-channel authorization for the real-time client: forwarded to Navree with the bearer token. */
export function POST(request: Request): Promise<Response> {
  return bff.proxy(request, "/broadcasting/auth");
}

export const dynamic = "force-dynamic";
