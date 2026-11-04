import type { Metadata } from "next";

import { Faq } from "@/components/faq";
import { pageMetadata } from "@/lib/metadata";
import { FOUNDER_ID, PageSchema } from "@/lib/structured-data";

import { Builder } from "./builder";
import { Contact } from "./contact";
import { AboutHero } from "./hero";
import { Lessons } from "./lessons";
import { TheName } from "./name";

export const metadata: Metadata = pageMetadata({
  title: "About Navree: who built it and why",
  description:
    "Navree is built by Amritpal Singh and named for his daughter, Navreet. The lessons of fifteen years and six startups, and the part of Navree each became.",
  path: "/about/",
});

/**
 * `/about/`: who built Navree, where the name comes from, what fifteen years
 * of building taught and where each lesson lives in Navree, and how to get in
 * touch. The words are in `src/content/about.ts`.
 */
export default function AboutPage() {
  return (
    <>
      <AboutHero />
      <TheName />
      <Builder />
      <Lessons />
      <Contact />
      <Faq name="about" />
      <PageSchema path="/about/" metadata={metadata} mainEntity={FOUNDER_ID} />
    </>
  );
}
