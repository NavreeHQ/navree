import { navree } from "./lib/navree";

export const proxy = navree.proxy({ loginPath: "/login", publicPaths: ["/about"] });

export const config = { matcher: ["/((?!_next/|favicon.ico).*)"] };
