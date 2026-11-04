import { navree } from "@/lib/navree";
import { socialCallback } from "@/lib/social";

/**
 * The provider sends the browser back here (`social.redirect_base` in the API's
 * config/app.toml is this route without the provider).
 */
export async function GET(request: Request, { params }: { params: Promise<{ provider: string }> }): Promise<Response> {
  const { provider } = await params;
  return socialCallback(navree.bff(), request, provider);
}

export const dynamic = "force-dynamic";
