import { bff } from "@/lib/bff/server";

/** Revokes the refresh token at Navree (`{all: true}` signs out every device) and clears the cookies. */
export function POST(request: Request): Promise<Response> {
  return bff.logout(request);
}

export const dynamic = "force-dynamic";
