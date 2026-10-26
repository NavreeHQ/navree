import { bff } from "@/lib/bff/server";

/** Private-channel authorisation for the Pusher client: forwarded to Navree with the bearer token. */
export function POST(request: Request): Promise<Response> {
  return bff.proxy(request, "/broadcasting/auth");
}

export const dynamic = "force-dynamic";
