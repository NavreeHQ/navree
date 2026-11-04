import { navree } from "@/lib/navree";
import { socialRedirect } from "@/lib/social";

/** `/auth/social/github?next=/w/acme`: a "Continue with ..." button starts here. */
export async function GET(request: Request, { params }: { params: Promise<{ provider: string }> }): Promise<Response> {
  const { provider } = await params;
  return socialRedirect(navree.bff(), request, provider);
}

export const dynamic = "force-dynamic";
