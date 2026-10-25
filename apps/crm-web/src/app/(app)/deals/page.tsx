import type { Metadata } from "next";
import { Suspense } from "react";
import { DealsBoardView } from "./deals-board";

export const metadata: Metadata = { title: "Deals" };

export default function DealsPage() {
  return (
    <Suspense>
      <DealsBoardView />
    </Suspense>
  );
}
