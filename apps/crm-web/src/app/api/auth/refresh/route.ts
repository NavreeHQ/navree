import { bff } from "@/lib/bff/server";

/** Rotates the token cookies using the refresh cookie. The browser never sees either token. */
export function POST(request: Request): Promise<Response> {
  return bff.refreshSession(request);
}

export const dynamic = "force-dynamic";
