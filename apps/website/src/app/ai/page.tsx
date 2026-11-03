import type { Metadata } from "next";

import { pageMetadata } from "@/lib/metadata";
import { PageSchema } from "@/lib/structured-data";

import { AgentsHalf } from "./agents-half";
import { AiClosing } from "./closing";
import { AiHero } from "./hero";
import { ProductHalf } from "./product-half";

const description =
  "Agent skills, the Navigator MCP server and docs written for models help your agent build Navree apps. An AI SDK, RAG and MCP servers serve your product.";

export const metadata: Metadata = pageMetadata({ title: "Navree for AI: skills, MCP and the AI SDK", description, path: "/ai/" });

export default function AiPage() {
  return (
    <>
      <AiHero />
      <AgentsHalf />
      <ProductHalf />
      <AiClosing />
      <PageSchema path="/ai/" metadata={metadata} />
    </>
  );
}
