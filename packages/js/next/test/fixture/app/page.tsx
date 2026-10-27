import { Suspense } from "react";
import { navree } from "../lib/navree";
import { DealsTable } from "./deals-table";

export const dynamic = "force-dynamic";

export default async function Home() {
  // The API is unreachable in the smoke test, so auth() can't confirm the user; it must not crash the page.
  const session = await navree.auth().catch(() => null);
  return (
    <main>
      <h1>Deals</h1>
      <p id="session">{session ? "signed in" : "unconfirmed"}</p>
      <Suspense fallback={<p>Loading</p>}>
        <DealsTable />
      </Suspense>
    </main>
  );
}
