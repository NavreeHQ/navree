import { bff } from "@/lib/bff/server";

/** Signs in at Navis, keeps the tokens in httpOnly cookies, returns `{user}`. */
export function POST(request: Request): Promise<Response> {
  return bff.login(request);
}

export const dynamic = "force-dynamic";
