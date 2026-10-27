import { bff } from "@/lib/bff/server";

/**
 * Every other `/api/*` call from the browser goes to the Navree API with the bearer token
 * from the httpOnly cookie (and the browser client's `X-Workspace`).
 */
function handle(request: Request): Promise<Response> {
  return bff.proxy(request, new URL(request.url).pathname);
}

export const dynamic = "force-dynamic";
export { handle as DELETE, handle as GET, handle as HEAD, handle as PATCH, handle as POST, handle as PUT };
