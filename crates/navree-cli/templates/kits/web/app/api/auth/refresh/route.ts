import { bff } from "@/lib/bff/server";

/** Rotates the token cookies (204), or ends the session (401). */
export function POST(request: Request): Promise<Response> {
  return bff.refreshSession(request);
}

export const dynamic = "force-dynamic";
