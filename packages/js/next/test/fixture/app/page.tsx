import { unwrap } from "@navree/client";
import { Suspense } from "react";
import { navree } from "../lib/navree";
import { DealsTable } from "./deals-table";

export const dynamic = "force-dynamic";

type Paths = {
  "/api/v1/deals": {
    get: {
      parameters: { query?: never };
      responses: { 200: { content: { "application/json": { auth: string | null; workspace: string | null } } } };
    };
  };
};

export default async function Home() {
  const session = await navree.auth<{ name: string }>().catch(() => null);
  const api = await navree.client<Paths>();
  const echo = await unwrap(api.GET("/api/v1/deals")).catch(() => null);
  return (
    <main>
      <h1>Deals</h1>
      <p id="session">{session ? `user:${session.user.name}` : "no user"}</p>
      <p id="echo">{echo ? `ws:${echo.workspace} auth:${echo.auth}` : "no deals"}</p>
      <Suspense fallback={<p>Loading</p>}>
        <DealsTable />
      </Suspense>
    </main>
  );
}
