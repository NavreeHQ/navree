import { navree } from "@/lib/navree";

/** Private-channel authorisation for the real-time client: forwarded to Navree with the bearer token. */
export const dynamic = "force-dynamic";
export const { POST } = navree.handlers;
