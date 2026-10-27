import { bff } from "@/lib/bff/server";

/**
 * The Helm admin panel (`/admin`, for super admins), served by the API and forwarded with the
 * same session, so it opens on this origin.
 */
function handle(request: Request): Promise<Response> {
  return bff.admin(request);
}

export const dynamic = "force-dynamic";
export { handle as DELETE, handle as GET, handle as HEAD, handle as PATCH, handle as POST, handle as PUT };
