# Writing the Navree docs

This is the guide for everyone writing the docs pages. The pages are MDX files
in the `docs/` folder of the framework repository, beside the code they
describe: one pull request changes the code and its docs. It covers
where pages live, the front-matter, every component you can use, how to
write code blocks, and the voice the docs are written in.

## Checking your pages

The docs are checked in two places.

- **In this repository**, the tests that read the docs run with
  `cargo test`. They fail when a page names a type, method, config key or
  command that does not exist, or when a Rust block no longer compiles. The
  agent skills' tests check their links into the docs the same way.
- **On navree.com**, the site is built from this folder. The build fails if a
  page in the manifest is missing, has no title or description, or isn't
  valid MDX, and `node scripts/check-docs.ts docs/basics/validation.mdx`
  (from the site's checkout) checks the front-matter, the components, the
  code languages and the links of the pages you name. A maintainer runs both
  before a change is published.

Every component below is shown rendered on the site's `/styleguide/` page,
whose source is `content/styleguide.mdx` there.

## Where pages live

Every page is an MDX file:

```
docs/<section>/<page>.mdx   →   /docs/<section>/<page>/
```

For example, `docs/basics/validation.mdx` is served at
`/docs/basics/validation/`.

**The navigation manifest is `docs/manifest.ts`.** It lists the
sections in sidebar order and each section's pages in order. The sidebar,
the previous/next links, the search index and the list of pages that get
built all come from it.

- A page that is in the manifest but has no file fails the build.
- A file that isn't in the manifest is not published (the build prints a
  warning).
- To add a page, create the file, then add its slug to its section in the
  manifest. To move or rename a page, change both, and search the docs for
  links to the old URL.
- Every section also has a page of its own, `/docs/<section>/`, that lists
  the section's pages with their descriptions. Give the section a
  `description` of 70 to 160 characters (it is the page's lead and its meta
  description; `node scripts/check-docs.ts` checks the length) and, for a
  long section, `groups` that cut the list under headings. The page carries
  its structured data (a `CollectionPage` that holds the list of pages), a
  Markdown copy at `/docs/<section>.md` and a line under the section in
  `llms.txt`.

## Front-matter

Every page starts with two fields:

```mdx
---
title: "Validation"
description: "`#[derive(Validate)]` rules and validated extractors, with field-level error messages."
---
```

- **`title`** is the page's `<h1>`, its sidebar label and its browser tab
  title. Use sentence case and keep it short: "Routing and controllers",
  not "How to Define Routes".
- **`description`** is one sentence, shown under the title, in search
  results and in link previews. Say what the reader will find, not that the
  page exists. Backticks work: `` `Valid<Json<T>>` ``.

### Pages for packages in development

A page about a package or tool that isn't in the repository yet adds a
third field:

```mdx
---
title: "Fleet"
description: "A queue dashboard and worker supervisor."
status: "planned"
---
```

- **Write it as finished documentation** of a shipped package:
  installation (the facade feature or the crate), configuration (config
  keys and environment variables), the API with realistic examples,
  testing, and a reference table where useful. Follow Navree's existing
  conventions exactly (`for_root`, `from_config` and `for_feature`, config
  key and env var names, extractors, errors, fakes) and use the names,
  crates and commands of the ecosystem catalogue
  (`src/content/ecosystem.ts`).
- **Don't claim its code compiles.** Samples must be consistent with each
  other and with the rest of Navree, but they can't be checked yet.
- **The site decides how it shows.** `showPlannedBadges` in
  `site.config.ts` adds an "In development" badge to every planned page and
  ecosystem card when it is `true`; while it is `false`, planned pages look
  like every other page. Remove the field when the package is built and the
  page has been checked against its code.
- Pages without `status` document built code: every type, function,
  attribute, config key and command they name must exist.

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
  `[custom messages](/docs/basics/validation/#custom-messages)`.
- The site's other pages are `/ecosystem/`, `/architecture/`, `/ai/` and
  `/performance/`. Link to Navree's measurements with
  `[performance](/performance/)`, never `/benchmarks/`: that URL only
  redirects to `/performance/`.
- External links use full URLs: `[Hyper](https://docs.rs/hyper)`.

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
| CSS | `css` |
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
Navree::app()
    .module(ConfigModule::for_root(ConfigSources::defaults()))
    .module(HttpModule::from_config())
    .module(QueueModule::from_config())
    .run()
```

```toml
navree = { version = "1", features = ["http", "db"] } # [!code --]
navree = { version = "1", features = ["postgres"] } # [!code ++]
tokio = { version = "1", features = ["full"] } # [!code highlight]
```
````

`[!code highlight]`, `[!code ++]` and `[!code --]` work in any language's
comment syntax (`//`, `#`, `--`) and are removed from the rendered code.

**Long lines.** Code scrolls sideways. For log lines, error messages, mail
text and long one-line JSON or headers, add `wrap` after the language
(` ```text wrap `, ` ```json title="400 Bad Request" wrap `) so long lines
wrap with a hanging indent instead. Don't use it for code, tables or
diagrams, whose alignment matters.

**Copying.** Every block has a copy button. So that what readers copy runs:

- Don't put `$` prompts in shell blocks.
- Show command output in a separate `text` block, not mixed with the
  commands.
- Keep comments in shell blocks short, after the command.

**Rust snippets must be real.** Every snippet uses real APIs. A non-trivial
snippet must compile against the workspace: check it in a scratch crate or a
`tests/` example, or quote it from the existing tests or the CRM
(`crm/api` in the CRM repository). Leave out the `use` lines only when they're obvious from
the page.

## Components

Components work without an import. Put the opening and closing tags on their
own lines and **leave blank lines around paragraphs, lists and code blocks
inside them**, as the examples do, so MDX parses each block as Markdown.

### Callout

A note, tip or warning set apart from the text. The title is optional.

```mdx
<Callout type="tip" title="Run one crate's tests">

Use `cargo test -p navree-validation` while you work on one crate.

</Callout>
```

- `note`: extra context the reader can skip.
- `tip`: a better way to do something.
- `warning`: something that loses data, breaks security or surprises
  people. Use it sparingly, or readers learn to skip it.

### Tabs

Alternatives the reader chooses between: PostgreSQL or SQLite, Cargo or the
CLI. Each tab can hold any Markdown, code blocks included.

```mdx
<Tabs>
<Tab label="PostgreSQL">

Turn on the `postgres` feature, then set `database.url`.

</Tab>
<Tab label="SQLite">

SQLite is opt-in: turn on the `sqlite` feature. There is nothing to install.

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
cargo install navree-cli
```

</Step>
<Step title="Create an app">

`navree new blog` writes a working app with one module.

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
`navree` facade a page or section needs. Put it at the start of the first
paragraph, not in the heading (it would end up in the heading's link id):

```mdx
## Queued listeners

<Badge feature="queue" /> A listener can run on a queue worker instead of in the request.

<Badge>Since 1.0</Badge> <Badge tone="accent">New</Badge> <Badge tone="warning">Experimental</Badge>
```

### LinkCards

A grid of links to related pages, for the end of a page or an overview.
Internal links use paths; links starting with `http` open the other site.

```mdx
<LinkCards>
  <LinkCard href="/docs/basics/validation/" title="Validation">
    `#[derive(Validate)]` rules and validated extractors, with field-level messages.
  </LinkCard>
  <LinkCard href="/docs/digging-deeper/queues/" title="Queues">
    Background jobs with retries and backoff.
  </LinkCard>
</LinkCards>
```

### Diagram

An architecture diagram, drawn as SVG and laid out again for phones. Use one
where a picture explains the page better than a paragraph:

```mdx
<Diagram name="request-lifecycle" />
```

The names are `modular-monolith`, `microservices`, `multi-tenancy`,
`event-driven`, `api-first`, `backend-for-frontend`, `request-lifecycle`,
`module-graph`, `navree-ui`, `reverse-engineering`, `design-to-ui`,
`two-ways`, `workspace-resolution`, `workspace-isolation`, `harbor-release`,
`orm-parts`, `jobs-and-events`, `transport-choice`, `token-and-session`,
`request-to-pixels` and `server-rendered-app`. The diagrams live in `src/components/diagrams/`, each
with a title and a text description for screen readers; to add one, add a
component there and its name to `src/components/mdx/diagram.tsx`. Put the
tag on its own line with blank lines around it, and don't repeat in prose
what the diagram already labels.

### ToolMock

A picture of one of the ecosystem tools, drawn from the same mock data as the
ecosystem page:

```mdx
<ToolMock name="fleet" />
```

The names are `fleet`, `harbor`, `helm`, `lighthouse` and `logbook`. The mocks
live in `src/components/ecosystem/`, and the list of names is in
`src/components/mdx/tool-mock.tsx`.

### The UI docs' components

The pages under `/docs/ui/` and `/docs/components/` show Navree UI with
components that read `docs/ui-data/`. That folder is written by
`cargo run -p ui-docs` from the toolkit itself, and `cargo test -p ui-docs`
fails when it is stale, so nothing on these pages is drawn by hand.

- `<UiPreview example="button/variants" />`: a live preview of an example,
  with Preview and Code tabs. Examples are Rust functions in
  `apps/ui-docs/src/examples/`; the function's body is the code shown.
  The preview is a static page rendered by navree-ui with its own stylesheet,
  framed, so the toolkit's CSS and the site's never meet. Add `defer` to a
  whole page drawn below the fold: its frame then loads after the page has.
  A preview has no server: a link, a submit or a request in it shows a short
  note saying what the app would do instead, and nothing leaves the frame.
  A popup that opens in a preview grows the frame while it is open, so give
  an example that opens a list a `.height()` with room for it.
- `<UiPreview example="theme-builder/default" live="/demo/ui/themes/builder?base=ocean&mode=dark" />`:
  with `live`, the frame shows that page of the live theme demo when the demo
  answers, and the example's picture, with a line saying the demo is down,
  when it doesn't. See "The live theme demo" below.
- `<UiAnatomy component="button" />`: the component's `data-slot` parts.
- `<UiProps type="Button" />`: a type's constructors and methods, or an
  enum's variants, read from the toolkit's source.
- `<UiChoose name="overlay" current="Dialog" />`: a selection diagram from
  `src/components/ui-docs/choices.ts`.
- `<UiA11y>` with a keyboard table and notes in Markdown inside.
- `<UiGrid />`, `<UiAssets />`, `<UiBudgets />`, `<UiTokens />` and
  `<UiCsp />`: the components as a grid of previews, the toolkit's files and
  their sizes, the size ceilings its tests hold (read from
  `crates/navree-ui/tests/budgets.rs`, with the size now), its colour tokens
  and its content security policy.

To add an example, write the function, list it in its module's `all()`,
run `cargo run -p ui-docs`, then use its id in a page.

After `pnpm build`, `pnpm check:previews` opens every popup, list, picker,
menu and dialog in every preview at a desktop and a phone width, and fails
on one that is cut by its frame, covers its own control or makes the frame
jump, and on any console error or failed request. `pnpm check:previews
--measure` also writes the height each preview settles at to
`src/components/ui-docs/heights.json`; build again and the frames start at
that height, so the page doesn't shift as they load.

### The live theme demo

The theme builder needs a server, so its docs page frames a small Navree
app, `apps/theme-demo`, served at `navree.com/demo/ui/`. It has the
builder, the themes and the layouts and nothing else, and keeps nothing:
a draft lives in its address, the theme menu's choice in the reader's
cookie, and Save and Import check a theme without storing it. It refuses a
post from another site, allows 300 requests a minute per visitor, and only
navree.com may frame it.

The docs ask `/demo/ui/health` first and load the live frame only when the
demo answers; otherwise, and whenever a page in the frame isn't one of the
demo's own, they show the builder's picture. To try it locally:

```sh
cargo run -p theme-demo                          # http://127.0.0.1:8095/demo/ui/themes/builder
```

The website serves the site with `/demo/ui/` passed to the demo: in the
website repository, `pnpm build`, then
`DEMO_UI=http://127.0.0.1:8095 pnpm serve`.

On the server, the website repository's `deploy/theme-demo.sh` builds the
demo and runs it under pm2 as `navree-theme-demo` on `127.0.0.1:8095`, with
no sudo; `deploy/demo-ui-setup.sh`, run once with sudo, adds the nginx
locations for `/demo/ui/` and `/_navree/ui/` and the request limit.

### Plain Markdown

Tables (GitHub style), lists, block quotes, bold, inline code and images all
work. Tables scroll sideways on phones, so a wide table is fine.

## Voice

The docs are clear, direct and friendly, written for a developer who wants
to get something done.

- **Say what Navree is and does, in its own terms.** Never compare it with
  another framework or use one as a reference point: no "like X", "X-style",
  "the way you know from X" or "X calls these ...". Describe the feature,
  its behaviour and its limits. Naming the libraries Navree is built on is
  good (Tokio, Hyper, Tower, sqlx, tonic, rustls,
  Argon2), and so are protocols and standards: "speaks the Pusher protocol,
  so pusher-js and any Pusher client connects", OpenAPI 3.1, OAuth 2.1,
  OpenTelemetry, MCP. The JSON error shape
  (`{ "statusCode", "message", "error", "code" }`) is Navree's.

- **Talk to the reader.** "You", never "the user" or "we". "Register the job
  in your module", not "The job should be registered".
- **Short sentences, active voice, present tense.** One idea per sentence.
  "`navree new` creates the app", not "The app will be created by running
  `navree new`".
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
- **Spelling.** Keep the spelling a page already uses (most existing pages
  are British: behaviour, organise); new pages use American spelling
  (behavior, organize). Either way, use the code's spelling for anything
  that is a name in the code (`serialize`, `Authorization`).
- **Confident, concrete, short.** Prefer verbs and numbers to adjectives.
  No exclamation marks, and none of "blazing", "revolutionary", "next-gen",
  "seamless", "robust", "leverage", "unlock" or "supercharge".
- **Limits are behaviour.** On pages about built code, state what a
  feature doesn't do as plainly as what it does ("counters are kept per
  process"). Point forward-looking work at the matching section of the
  roadmap, such as `/docs/prologue/roadmap/#what-the-other-items-mean`. Packages that
  aren't built yet get a `status: "planned"` page (see
  [Front-matter](#pages-for-packages-in-development)), not a caveat.

