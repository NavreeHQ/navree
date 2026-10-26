import { InlineMarkdown } from "@/components/inline-markdown";
import { BlurFade } from "@/components/magicui/blur-fade";
import { highlight } from "@/lib/mdx";

import { HalfHeading } from "./parts";
import { ProductTabs, type ProductPanel } from "./product-tabs";
import { productSamples, providers } from "./samples";

/** The principles from /docs/ai/introduction/#principles. */
const principles: { title: string; text: string }[] = [
  {
    title: "Models act as users",
    text: "Agent and MCP tools run with the permissions of the user they act for, and every write goes through your services, your access rules and your audit log.",
  },
  {
    title: "Providers are configuration",
    text: "Code names a model by its job, such as `fast` or `embed`, and `config/app.toml` maps it to a provider. Moving vendors, or to Ollama on your own hardware, changes no code.",
  },
  {
    title: "Your data stays yours",
    text: "Nothing goes to a provider unless you configure one. Prompts and outputs aren't logged unless you turn it on, and secrets are redacted wherever Navree prints configuration.",
  },
  {
    title: "Tested like everything else",
    text: "`AiFake` answers in tests without a network or a key, records every request for assertions, and lets your tools run for real against the test database.",
  },
];

/** Shiki HTML with a focusable `<pre>`, so keyboard users can scroll long lines. */
async function code(source: string, lang: string): Promise<string> {
  const html = await highlight(source, lang);
  return html.replace("<pre ", '<pre tabindex="0" ');
}

export async function ProductHalf() {
  const panels: ProductPanel[] = await Promise.all(
    productSamples.map(async (sample) => ({
      id: sample.id,
      label: sample.label,
      hint: sample.hint,
      title: sample.title,
      text: sample.text,
      points: sample.points,
      href: sample.href,
      linkText: sample.linkText,
      file: sample.file,
      html: await code(sample.code, "rust"),
      ...(sample.extra
        ? { extra: { file: sample.extra.file, html: await code(sample.extra.code, sample.extra.lang) } }
        : {}),
    })),
  );

  return (
    <section id="product" aria-labelledby="product-title" className="border-t border-line">
      <div className="container-site section-y">
        <HalfHeading
          id="product-title"
          index="02"
          eyebrow="Build AI into your product"
          title="Models are a service like any other."
          lead={
            <p>
              <code className="inline-code">AiModule</code> puts every major model provider behind one typed service:
              named in configuration, injected where you need it, faked in tests. Agents act through your services as the
              signed-in user, retrieval answers from your own data, and MCP servers put your app in any AI client.
            </p>
          }
        />

        {/* The providers, as plain names: one service in front of all of them. */}
        <div className="mt-12 flex flex-wrap items-center gap-x-6 gap-y-3 border-y border-line py-5 lg:mt-14">
          <span className="font-mono text-xs tracking-[0.12em] text-fg-3 uppercase">One service for</span>
          <ul className="flex flex-wrap items-center gap-x-6 gap-y-2">
            {providers.map((name) => (
              <li key={name} className="font-display text-[1.0625rem] font-semibold tracking-[-0.01em] text-fg-1">
                {name}
              </li>
            ))}
          </ul>
        </div>

        <div className="mt-14 lg:mt-20">
          <ProductTabs panels={panels} />
        </div>

        <BlurFade inView className="mt-24 lg:mt-32">
          <h3 className="sr-only">Principles</h3>
          <ul className="grid grid-cols-1 gap-px overflow-hidden rounded-2xl border border-line bg-line md:grid-cols-2 xl:grid-cols-4">
            {principles.map((p, i) => (
              <li key={p.title} className="bg-ink-0 p-6 sm:p-7">
                <p className="font-mono text-xs text-fg-3 numeric">{String(i + 1).padStart(2, "0")}</p>
                <p className="mt-3 font-display text-display-sm font-semibold text-fg">{p.title}</p>
                <p className="mt-3 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
                  <InlineMarkdown text={p.text} />
                </p>
              </li>
            ))}
          </ul>
        </BlurFade>
      </div>
    </section>
  );
}
