# Writing the Navis docs

This is the guide for everyone writing pages in `apps/website`. It covers
where pages live, the front-matter, every component you can use, how to
write code blocks, and the voice the docs are written in.

## Run the site while you write

```sh
cd apps/website
pnpm install
pnpm dev            # http://localhost:3210, reloads when you save
```

- Open **http://localhost:3210/styleguide/** to see every component and
  Markdown element rendered with the site's styles. Its source,
  `content/styleguide.mdx`, is a good file to copy examples from.
- Before you finish, run `pnpm build`. It fails if a page in the manifest is
  missing, has no title or description, or isn't valid MDX.

## Where pages live

Every page is an MDX file:

```
content/docs/<section>/<page>.mdx   →   /docs/<section>/<page>/
```

For example, `content/docs/basics/validation.mdx` is served at
`/docs/basics/validation/`.

**The navigation manifest is `content/docs/manifest.ts`.** It lists the
sections in sidebar order and each section's pages in order. The sidebar,
the previous/next links, the search index and the list of pages that get
built all come from it.

- A page that is in the manifest but has no file fails the build.
- A file that isn't in the manifest is not published (the build prints a
  warning).
- To add a page, create the file, then add its slug to its section in the
  manifest. To move or rename a page, change both, and search the docs for
  links to the old URL.

Every page from the design (§3) already exists as a stub. Each stub has its
front-matter, a summary of what the page will cover and a callout titled
"This page is being written". **Write into the existing file and delete the
callout** when the page is done. Keep the file names: other pages link to
them.

The comments in the manifest name each section's content owner (A to F).
"Digging deeper" has two owners; the comment above each group of pages says
whose they are.

## Front-matter

Every page starts with two fields:

```mdx
---
title: "Validation"
description: "Validated extractors on garde, with field-level error messages."
---
```

- **`title`** is the page's `<h1>`, its sidebar label and its browser tab
  title. Use sentence case and keep it short: "Routing and controllers",
  not "How to Define Routes".
- **`description`** is one sentence, shown under the title, in search
  results and in link previews. Say what the reader will find, not that the
  page exists. Backticks work: `` `Valid<Json<T>>` ``.

Don't write an `# h1` in the body: the title is the page's heading.

## Headings and links

- Start sections at `##`, with `###` for subsections. "On this page" lists
  both. Use `####` rarely: it isn't listed.
- Every heading becomes a link target. The id is the heading text in lower
  case, punctuation removed and spaces turned into hyphens:
  `## Custom messages` is `#custom-messages`. A repeated heading gets `-1`,
  `-2` and so on.
- Link to other pages with absolute paths and a trailing slash:
  `[validation](/docs/basics/validation/)` or
  `[custom messages](/docs/basics/validation/#custom-messages)`. Link to the
  benchmarks with `/benchmarks/`.
- External links use full URLs: `[garde](https://docs.rs/garde)`.

## Code blocks

Always name the language after the opening fence. Highlighting happens at
build time with Shiki.

| Language | Fence names |
|---|---|
| Rust | `rust`, `rs` |
| TOML | `toml` |
| Shell | `sh`, `bash`, `shell`, `console` |
| TypeScript | `ts`, `typescript`, `tsx` |
| JSON | `json` |
| SQL | `sql` |
| HTTP | `http` |
| HTML and templates | `html`, `jinja` |
| YAML and `.env` | `yaml`, `env` |
| Anything else | `text` |

**Filename titles.** Add `title="..."` after the language. The title replaces
the language in the block's bar. Use it whenever the code belongs in a file:

````mdx
```rust title="src/deals/controller.rs"
#[controller("/deals", version = "1")]
impl DealsController { /* ... */ }
```
````

**Highlighting lines.** Put the line numbers in braces after the language
(`{3}`, `{2-4}`, `{1,5-7}`), or mark a line in place with a comment:

````mdx
```rust title="src/app.rs" {4}
Navis::app()
    .module(ConfigModule::for_root(ConfigSources::defaults()))
    .module(HttpModule::from_config())
    .module(QueueModule::from_config())
    .run()
```

```toml
navis = { version = "0.1", features = ["http", "db"] } # [!code --]
navis = { version = "0.1", features = ["postgres"] } # [!code ++]
tokio = { version = "1", features = ["full"] } # [!code highlight]
```
````

`[!code highlight]`, `[!code ++]` and `[!code --]` work in any language's
comment syntax (`//`, `#`, `--`) and are removed from the rendered code.

**Copying.** Every block has a copy button. So that what readers copy runs:

- Don't put `$` prompts in shell blocks.
- Show command output in a separate `text` block, not mixed with the
  commands.
- Keep comments in shell blocks short, after the command.

**Rust snippets must be real.** Every snippet uses real APIs. A non-trivial
snippet must compile against the workspace: check it in a scratch crate or a
`tests/` example, or quote it from the existing tests or the CRM
(`apps/crm-api`). Leave out the `use` lines only when they're obvious from
the page.

## Components

Components work without an import. Put the opening and closing tags on their
own lines and **leave blank lines around paragraphs, lists and code blocks
inside them**, as the examples do, so MDX parses each block as Markdown.

### Callout

A note, tip or warning set apart from the text. The title is optional.

```mdx
<Callout type="tip" title="Run one crate's tests">

Use `cargo test -p navis-validation` while you work on one crate.

</Callout>
```

- `note`: extra context the reader can skip.
- `tip`: a better way to do something.
- `warning`: something that loses data, breaks security or surprises
  people. Use it sparingly, or readers learn to skip it.

### Tabs

Alternatives the reader chooses between: SQLite or PostgreSQL, Cargo or the
CLI. Each tab can hold any Markdown, code blocks included.

```mdx
<Tabs>
<Tab label="SQLite">

SQLite is the default. There is nothing to install.

</Tab>
<Tab label="PostgreSQL">

Turn on the `postgres` feature, then set `database.url`.

</Tab>
</Tabs>
```

### CodeGroup

Several related files in one block, one tab per file. Each tab is named
after its block's `title`.

````mdx
<CodeGroup>

```rust title="src/main.rs"
fn main() -> Result<(), BoxError> {
    app::app(ConfigSources::defaults()).run()
}
```

```toml title="config/app.toml"
[http]
addr = "127.0.0.1:8080"
```

</CodeGroup>
````

### Steps

A numbered procedure. Use it only when the order matters.

````mdx
<Steps>
<Step title="Install the CLI">

```sh
cargo install navis-cli
```

</Step>
<Step title="Create an app">

`navis new blog` writes a working app with one module.

</Step>
</Steps>
````

### FileTree

A directory listing. Indent two spaces per level, end directory names with
`/`, add a comment after `#`, and end an entry with `*` to highlight it.
Write the tree inside `{`...`}` so MDX keeps the indentation.

```mdx
<FileTree title="A new app">{`
Cargo.toml
config/
  app.toml          # settings
src/
  main.rs           # boots the app
  modules/
    deals/ *
      mod.rs
`}</FileTree>
```

### Badge

A small inline label. Use `feature` to say which Cargo feature of the
`navis` facade a page or section needs. Put it at the start of the first
paragraph, not in the heading (it would end up in the heading's link id):

```mdx
## Queued listeners

<Badge feature="queue" /> A listener can run on a queue worker instead of in the request.

<Badge>Since 0.1</Badge> <Badge tone="accent">New</Badge> <Badge tone="warning">Experimental</Badge>
```

### LinkCards

A grid of links to related pages, for the end of a page or an overview.
Internal links use paths; links starting with `http` open the other site.

```mdx
<LinkCards>
  <LinkCard href="/docs/basics/validation/" title="Validation">
    Validated extractors on garde, with field-level messages.
  </LinkCard>
  <LinkCard href="/docs/digging-deeper/queues/" title="Queues">
    Background jobs with retries and backoff.
  </LinkCard>
</LinkCards>
```

### Plain Markdown

Tables (GitHub style), lists, block quotes, bold, inline code and images all
work. Tables scroll sideways on phones, so a wide table is fine.

## Voice

The docs read like Laravel's: clear, direct and friendly, written for a
developer who wants to get something done.

- **Talk to the reader.** "You", never "the user" or "we". "Register the job
  in your module", not "The job should be registered".
- **Short sentences, active voice, present tense.** One idea per sentence.
  "`navis new` creates the app", not "The app will be created by running
  `navis new`".
- **Say why, then how.** Open a section with what the feature is for, in a
  sentence or two, then show the code. Show the most common case first and
  the options after it.
- **Show, don't describe.** A short, real example beats a paragraph about
  one. Follow a code block with what it does, not with a restatement of the
  code.
- **Be exact.** Name types, methods, commands and config keys as they are in
  the code, in backticks: `Valid<Json<T>>`, `queue:work`, `database.url`.
  State defaults and limits when they matter ("refresh tokens last 30 days").
- **No filler.** Leave out "simply", "just", "easy", "obviously", "note
  that", "in order to" and "please". If something is easy, the example shows
  it.
- **Sentence case** for titles and headings: "Rotating refresh tokens", not
  "Rotating Refresh Tokens".
- **Spelling.** British in prose (behaviour, organise), but use the code's
  spelling for anything that is a name in the code (`serialize`,
  `Authorization`).
- **Be honest about limits.** If something isn't built yet (Redis drivers,
  passkeys), say so and link to the roadmap. Don't document plans as
  features.

## Building and deploying the site

```sh
pnpm lint          # ESLint
pnpm typecheck     # tsc (runs the prebuild first)
pnpm build         # static export to out/
pnpm serve         # serves out/ at http://127.0.0.1:3211, like a static host
```

`pnpm build` first runs `scripts/prebuild.ts`, which:

- reads `bench/results/*.json`, `bench/results/extra/`, `bench/frameworks.json`
  and each implementation's README into `src/generated/benchmarks.json`, and
  copies the raw results to `public/data/bench/` (every number on the site
  comes from there, so re-running the benchmark updates the site on the next
  build);
- reads the crate layers from `crates/navis/tests/dependency_graph.rs` and
  the descriptions from each `Cargo.toml` into `src/generated/crates.json`;
- checks the docs against the manifest and builds the search index into
  `public/data/search-index.json`.

`out/` is plain HTML, CSS and JavaScript: upload it to any static host
(Cloudflare Pages, Vercel, GitHub Pages, S3, nginx). Every page is a
directory with an `index.html`, so no rewrite rules are needed; serve
`out/404.html` for missing pages. Site-wide strings (the name, the repository
URL, the install commands) are in `site.config.ts`.

`pnpm screenshots` saves full-page screenshots of the key pages at 1440 and
390 px to `screenshots/` (run `pnpm serve` first).
