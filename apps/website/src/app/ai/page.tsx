import type { Metadata } from "next";

import { pageMetadata } from "@/lib/metadata";

import { AgentsHalf } from "./agents-half";
import { AiClosing } from "./closing";
import { AiHero } from "./hero";
import { ProductHalf } from "./product-half";

const description =
  "Navree is AI-native on both sides: agent skills, Navigator and docs written for models for the agent that writes your code, with skills that bring an existing product or design into Navree; and an AI SDK, agents, RAG and MCP servers for your product.";

export const metadata: Metadata = pageMetadata({ title: "Navree for AI", description, path: "/ai/" });

export default function AiPage() {
  return (
    <>
      <AiHero />
      <AgentsHalf />
      <ProductHalf />
      <AiClosing />
    </>
  );
}
