import { bff } from "@/lib/bff/server";

/** `/auth/social/github?next=/w/acme`: a "Continue with ..." button starts here. */
export async function GET(request: Request, { params }: { params: Promise<{ provider: string }> }): Promise<Response> {
  const { provider } = await params;
  return bff.socialRedirect(request, provider);
}

export const dynamic = "force-dynamic";
