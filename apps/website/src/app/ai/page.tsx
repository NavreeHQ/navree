import type { Metadata } from "next";

import { AgentsHalf } from "./agents-half";
import { AiClosing } from "./closing";
import { AiHero } from "./hero";
import { ProductHalf } from "./product-half";

const description =
  "Navree is AI-native on both sides: agent skills, Navigator and docs written for models for the agent that writes your code, and an AI SDK, agents, RAG and MCP servers for your product.";

export const metadata: Metadata = {
  title: "Navree for AI",
  description,
  alternates: { canonical: "/ai/" },
  openGraph: { title: "Navree for AI", description, url: "/ai/", images: ["/og.png"] },
};

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
