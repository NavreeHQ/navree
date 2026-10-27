import { bff } from "@/lib/bff/server";

/** The link in the verification mail: verified at Navree, then the `/verify-email` page. */
export function GET(request: Request): Promise<Response> {
  return bff.verifyEmail(request);
}

export const dynamic = "force-dynamic";
