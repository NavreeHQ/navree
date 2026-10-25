import type { Metadata } from "next";
import { Suspense } from "react";
import { CompaniesView } from "./companies-view";

export const metadata: Metadata = { title: "Companies" };

export default function CompaniesPage() {
  return (
    <Suspense>
      <CompaniesView />
    </Suspense>
  );
}
