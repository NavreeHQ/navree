import { bff } from "@/lib/bff/server";

/** Every `/api/*` call from the browser is forwarded to Navree with the bearer token from the httpOnly cookie. */
function handle(request: Request): Promise<Response> {
  return bff.proxy(request, new URL(request.url).pathname);
}

export const dynamic = "force-dynamic";
export { handle as DELETE, handle as GET, handle as HEAD, handle as PATCH, handle as POST, handle as PUT };
