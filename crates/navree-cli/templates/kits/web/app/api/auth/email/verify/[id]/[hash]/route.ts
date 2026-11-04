import { navree } from "@/lib/navree";
import { verifyEmail } from "@/lib/social";

/** The link in the verification mail: verified at Navree, then the `/verify-email` page. */
export function GET(request: Request): Promise<Response> {
  return verifyEmail(navree.bff(), request);
}

export const dynamic = "force-dynamic";
