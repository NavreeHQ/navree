import type { Metadata } from "next";
import { DealDetail } from "./deal-detail";

export const metadata: Metadata = { title: "Deal" };

export default async function DealPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return <DealDetail id={id} />;
}
