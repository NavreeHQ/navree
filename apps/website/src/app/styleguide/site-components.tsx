import { BoxesIcon, GaugeIcon, LayersIcon, ShieldCheckIcon } from "lucide-react";
import type { ReactNode } from "react";

import { AnimatedGradientText } from "@/components/magicui/animated-gradient-text";
import { AnimatedList } from "@/components/magicui/animated-list";
import { BentoCard, BentoGrid } from "@/components/magicui/bento-grid";
import { BlurFade } from "@/components/magicui/blur-fade";
import { BorderBeam } from "@/components/magicui/border-beam";
import { DotPatternGlow } from "@/components/magicui/dot-pattern-glow";
import { FlickeringGrid } from "@/components/magicui/flickering-grid";
import { GridPattern } from "@/components/magicui/grid-pattern";
import { MagicCard } from "@/components/magicui/magic-card";
import { Marquee } from "@/components/magicui/marquee";
import { MorphingText } from "@/components/magicui/morphing-text";
import { NumberTicker } from "@/components/magicui/number-ticker";
import { OrbitingCircles } from "@/components/magicui/orbiting-circles";
import { ShineBorder } from "@/components/magicui/shine-border";
import { AnimatedSpan, Terminal, TypingAnimation } from "@/components/magicui/terminal";
import { TextAnimate } from "@/components/magicui/text-animate";
import { WordRotate } from "@/components/magicui/word-rotate";
import { PlannedBadge } from "@/components/site/planned-badge";
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from "@/components/ui/accordion";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

import { BeamDemo } from "./beam-demo";

function Demo({ name, file, children }: { name: string; file: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-4 border-t border-line pt-8">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <h3 className="text-[1.0625rem] font-semibold text-fg">{name}</h3>
        <code className="inline-code text-fg-2">{file}</code>
      </div>
      {children}
    </section>
  );
}

const FEED = [
  { id: "a", title: "PgPool timed out acquiring a connection", meta: "deals · 14 events" },
  { id: "b", title: "Slow request: GET /v1/deals took 1.8 s", meta: "api · p95" },
  { id: "c", title: "Job SendInvoice failed after 3 attempts", meta: "queue: billing" },
];

/** The shadcn/ui and Magic UI building blocks, as restyled for Navree. */
export function SiteComponents() {
  return (
    <div className="mt-20 flex flex-col gap-10">
      <div>
        <p className="eyebrow">Site components</p>
        <h2 className="mt-4 text-display-lg font-semibold text-fg">The building blocks</h2>
        <p className="mt-4 max-w-2xl text-fg-2">
          shadcn/ui primitives in <code className="inline-code">src/components/ui</code> and Magic UI pieces in{" "}
          <code className="inline-code">src/components/magicui</code>. DESIGN.md says when to use each.
        </p>
      </div>

      <Demo name="Type" file="font-display / font-sans / font-mono">
        <div className="flex flex-col gap-3">
          <p className="font-display text-display-xl font-semibold text-fg">The complete backend framework.</p>
          <p className="font-display text-display-md font-semibold text-fg">Modules wired by dependency injection.</p>
          <p className="text-lg text-fg-2">Geist for body copy and UI. Short sentences, concrete numbers.</p>
          <p className="font-mono text-sm text-fg-1">navree new blog --kit saas</p>
          <p className="eyebrow">Eyebrow label</p>
        </div>
      </Demo>

      <Demo name="Button, Badge, Tooltip" file="ui/button, ui/badge, ui/tooltip">
        <div className="flex flex-wrap items-center gap-3">
          <Button>Get started</Button>
          <Button variant="outline">Read the docs</Button>
          <Button variant="secondary">Secondary</Button>
          <Button variant="ghost">Ghost</Button>
          <Button variant="link">Link</Button>
          <Button size="sm">Small</Button>
          <Button size="lg">Large</Button>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Badge>Built in</Badge>
          <Badge variant="secondary">Secondary</Badge>
          <Badge variant="outline">Outline</Badge>
          <Badge variant="mono">navree-workspace</Badge>
          <Badge variant="warning">Warning</Badge>
          <PlannedBadge />
          <Tooltip>
            <TooltipTrigger asChild>
              <Button variant="outline" size="sm">
                Hover me
              </Button>
            </TooltipTrigger>
            <TooltipContent>Tooltips sit on ink-3 with a hairline.</TooltipContent>
          </Tooltip>
        </div>
      </Demo>

      <Demo name="Card, Tabs, Accordion, Separator" file="ui/card, ui/tabs, ui/accordion">
        <div className="grid gap-4 md:grid-cols-2">
          <Card>
            <CardHeader>
              <CardTitle>Workspaces</CardTitle>
              <CardDescription>Members, roles, invitations and isolation by row, schema or database.</CardDescription>
            </CardHeader>
            <CardContent>
              <Separator className="mb-4" />
              <Tabs defaultValue="row">
                <TabsList>
                  <TabsTrigger value="row">Row</TabsTrigger>
                  <TabsTrigger value="schema">Schema</TabsTrigger>
                  <TabsTrigger value="database">Database</TabsTrigger>
                </TabsList>
                <TabsContent value="row" className="text-sm text-fg-2">
                  A <code className="inline-code">workspace_id</code> column, enforced by row-level security.
                </TabsContent>
                <TabsContent value="schema" className="text-sm text-fg-2">
                  One PostgreSQL schema per workspace.
                </TabsContent>
                <TabsContent value="database" className="text-sm text-fg-2">
                  One database per workspace.
                </TabsContent>
              </Tabs>
            </CardContent>
          </Card>
          <div className="surface px-5">
            <Accordion type="single" collapsible defaultValue="a">
              <AccordionItem value="a">
                <AccordionTrigger>Is it free?</AccordionTrigger>
                <AccordionContent>Yes. Open source, MIT OR Apache-2.0, self-hosted.</AccordionContent>
              </AccordionItem>
              <AccordionItem value="b">
                <AccordionTrigger>Does it need a container runtime?</AccordionTrigger>
                <AccordionContent>No. One binary; Dock starts local services when you want them.</AccordionContent>
              </AccordionItem>
            </Accordion>
          </div>
        </div>
        <Tabs defaultValue="rust">
          <TabsList variant="line">
            <TabsTrigger value="rust">main.rs</TabsTrigger>
            <TabsTrigger value="toml">navree.toml</TabsTrigger>
          </TabsList>
        </Tabs>
      </Demo>

      <Demo name="Terminal" file="magicui/terminal">
        <div className="bezel max-w-xl">
          <Terminal title="~/projects" className="rounded-[calc(1.25rem-0.375rem)] border-line-2">
            <TypingAnimation>$ navree new blog</TypingAnimation>
            <AnimatedSpan className="text-accent">✓ Created blog/ with 4 modules</AnimatedSpan>
            <AnimatedSpan>✓ Wrote AGENTS.md and CLAUDE.md</AnimatedSpan>
            <TypingAnimation>$ cargo run</TypingAnimation>
            <AnimatedSpan>Listening on http://127.0.0.1:3000 (ready in 9 ms)</AnimatedSpan>
          </Terminal>
        </div>
      </Demo>

      <Demo name="Word rotate, Morphing text, Text animate, Gradient text" file="magicui/word-rotate, morphing-text, text-animate, animated-gradient-text">
        <p className="font-display text-display-md font-semibold text-fg">
          Build a <WordRotate className="text-accent" words={["modular monolith", "microservice", "multi-tenant SaaS", "BFF"]} />
        </p>
        <MorphingText className="text-display-lg" texts={["Modular", "API-first", "AI-native"]} />
        <TextAnimate animation="blurInUp" by="word" className="text-lg text-fg-2">
          If it compiles, it is wired.
        </TextAnimate>
        <p className="text-lg text-fg-1">
          <AnimatedGradientText className="font-semibold">AI-native</AnimatedGradientText> from the first command.
        </p>
      </Demo>

      <Demo name="Animated beam" file="magicui/animated-beam">
        <BeamDemo />
      </Demo>

      <Demo name="Bento grid, Magic card, Border beam, Shine border" file="magicui/bento-grid, magic-card, border-beam, shine-border">
        <BentoGrid className="lg:auto-rows-[15rem]">
          <BentoCard
            className="lg:col-span-4"
            name="Modular by design"
            description="Every feature is a module that declares what it imports, provides and exports."
            Icon={BoxesIcon}
            href="/docs/architecture/modules/"
            cta="Modules"
            background={<GridPattern className="[mask-image:linear-gradient(to_bottom,black,transparent)]" squares={[[2, 1], [5, 2], [9, 1]]} />}
          />
          <BentoCard
            className="lg:col-span-2"
            name="Secure by default"
            description="Routes deny until you allow them."
            Icon={ShieldCheckIcon}
          />
          <BentoCard className="lg:col-span-2" name="Fast and frugal" description="One binary, under 40 MB under load." Icon={GaugeIcon} />
          <BentoCard
            className="lg:col-span-4"
            name="Scales out"
            description="Split into services over NATS, Kafka, RabbitMQ, Redis Streams or gRPC."
            Icon={LayersIcon}
          />
        </BentoGrid>
        <div className="grid gap-4 md:grid-cols-3">
          <MagicCard className="rounded-2xl p-6">
            <p className="font-semibold text-fg">Magic card</p>
            <p className="mt-1 text-sm text-fg-2">The border lights up near the pointer.</p>
          </MagicCard>
          <div className="relative overflow-hidden rounded-2xl border border-line bg-ink-1 p-6">
            <p className="font-semibold text-fg">Border beam</p>
            <p className="mt-1 text-sm text-fg-2">A streak runs around the frame.</p>
            <BorderBeam />
          </div>
          <div className="relative rounded-2xl bg-ink-1 p-6">
            <ShineBorder />
            <p className="font-semibold text-fg">Shine border</p>
            <p className="mt-1 text-sm text-fg-2">A slow sheen on one frame.</p>
          </div>
        </div>
      </Demo>

      <Demo name="Marquee, Number ticker" file="magicui/marquee, number-ticker">
        <Marquee className="[--duration:30s] mask-x-from-85%">
          {["Lighthouse", "Logbook", "Fleet", "Helm", "Harbor", "Dock", "Navigator", "Search", "AI SDK", "Billing"].map((name) => (
            <span key={name} className="rounded-full border border-line bg-ink-1 px-4 py-2 text-sm text-fg-1">
              {name}
            </span>
          ))}
        </Marquee>
        <div className="flex flex-wrap gap-10">
          <p className="flex flex-col">
            <span className="font-display text-display-lg font-semibold text-fg">
              <NumberTicker value={30} /> MB
            </span>
            <span className="text-sm text-fg-3">memory under load</span>
          </p>
          <p className="flex flex-col">
            <span className="font-display text-display-lg font-semibold text-fg">
              <NumberTicker value={142318} />
            </span>
            <span className="text-sm text-fg-3">requests per second</span>
          </p>
        </div>
      </Demo>

      <Demo name="Dot pattern, Grid pattern, Flickering grid" file="magicui/dot-pattern, grid-pattern, flickering-grid">
        <div className="grid gap-4 md:grid-cols-3">
          <div className="relative h-40 overflow-hidden rounded-2xl border border-line">
            <DotPatternGlow glow className="[mask-image:radial-gradient(ellipse_at_center,black,transparent_75%)]" />
          </div>
          <div className="relative h-40 overflow-hidden rounded-2xl border border-line">
            <GridPattern width={32} height={32} squares={[[1, 1], [3, 2], [6, 1]]} className="[mask-image:radial-gradient(ellipse_at_center,black,transparent_80%)]" />
          </div>
          <div className="relative h-40 overflow-hidden rounded-2xl border border-line">
            <FlickeringGrid className="absolute inset-0" />
          </div>
        </div>
      </Demo>

      <Demo name="Animated list, Orbiting circles, Blur fade" file="magicui/animated-list, orbiting-circles, blur-fade">
        <div className="grid gap-4 md:grid-cols-2">
          <div className="h-64 overflow-hidden rounded-2xl border border-line bg-ink-0 p-4">
            <AnimatedList>
              {FEED.map((item) => (
                <div key={item.id} className="w-full rounded-xl border border-line bg-ink-1 px-4 py-3">
                  <p className="text-sm font-medium text-fg">{item.title}</p>
                  <p className="mt-0.5 font-mono text-xs text-fg-3">{item.meta}</p>
                </div>
              ))}
            </AnimatedList>
          </div>
          <div className="relative flex h-64 items-center justify-center overflow-hidden rounded-2xl border border-line bg-ink-0">
            <span className="z-10 rounded-full border border-accent bg-ink-2 px-3 py-1.5 font-mono text-xs text-accent shadow-glow-sm">AppModule</span>
            <OrbitingCircles radius={92} iconSize={56} duration={40}>
              {["auth", "queue", "mail", "search"].map((m) => (
                <span key={m} className="rounded-full border border-line-2 bg-ink-1 px-2 py-1 font-mono text-[0.6875rem] text-fg-1">
                  {m}
                </span>
              ))}
            </OrbitingCircles>
          </div>
        </div>
        <BlurFade inView>
          <p className="text-fg-2">This paragraph fades up out of a light blur as it scrolls into view.</p>
        </BlurFade>
      </Demo>
    </div>
  );
}
