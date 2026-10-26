# Brief for the Navree documentation writers

You write some of the pages of the Navree documentation (navree.com/docs).
Navree is the Rust web framework in this repository; it was built under the
working name Keel and briefly called Navis, so you may still meet those
names in the 2026-10-25 history notes. Always write **Navree** and `navree::`.

## Read first
1. `apps/website/AUTHORING.md`: page layout, front-matter, the MDX components, code-block conventions and the voice.
2. `docs/superpowers/specs/2026-10-26-navree-website-design.md` §3: the documentation plan and its accuracy rule.
3. The root `README.md`: the crate hierarchy and what each crate does.
4. `apps/website/content/docs/manifest.ts`: the page order. Link to pages as `/docs/<section>/<page>/`.

## The rules
- **Write from the code, not from memory.** For every page, read the crate(s) it covers: `crates/navree-*/src`, their tests (`crates/*/tests`), and how the reference app uses them (`apps/crm-api` and `apps/crm-api/crates/*`). Every type, function, macro attribute, config key, environment variable, CLI command and default you mention must exist, so grep for it before you write it down.
  - Config keys come from the config structs and their serde names and defaults (look for `#[serde(default...)]` and `Default` impls).
  - CLI commands come from `crates/navree-cli` and the commands modules register.
- **Show real code.** Short, realistic examples in the style of the CRM.
  - A non-trivial Rust example must compile. Check it in your own scratch crate, `target/docs-check/<your-label>/`: its `Cargo.toml` has an empty `[workspace]` table and depends on `navree = { path = "../../../crates/navree" }` (plus `tokio`, `serde` and so on as needed).
  - Build with `CARGO_TARGET_DIR=target/agents/docs-check CARGO_BUILD_JOBS=6 cargo check`. This target directory is **shared by all writers**: cargo queues you while another writer compiles, and you reuse each other's compiled dependencies. Wait patiently and never delete it.
  - Snippets may leave out obvious parts with `// ...`, but the visible lines must be correct.
- **Be honest about limits.** If something isn't supported, is only partly supported, or was deferred (see the design spec of 2026-10-25 §2 and `docs/superpowers/notes/framework-open-issues.md`), say so plainly instead of implying it works. No invented features, versions or benchmarks.
- **Cover the topic completely**, the way Laravel's docs page for the same topic would:
  - an introduction;
  - the common path first, then configuration, options and edge cases;
  - testing tips where relevant;
  - cross-links to related pages.

  Use `##` and `###` headings; they feed the "On this page" table of contents.
- **Replace the stub completely**, including the "This page is being written" callout. Keep the front-matter `title`; improve the `description` if you can.
- **Stay in your lane.** Edit only your own pages: no other pages, no components, not the manifest. If a page needs an image, a component or a manifest change, say so in your final answer instead.
  - Don't run `pnpm build`, `pnpm dev` or `pnpm typecheck` in `apps/website` (several writers share it).
  - Check your pages with `node scripts/check-docs.ts <your files>` from `apps/website` instead. It must print `OK`.
- **Process.** Don't commit. Never use SendMessage to contact other agents. Don't change any code outside `target/docs-check/`.
  - If you find a bug in Navree while writing, don't fix it: describe it in your final answer (file, what's wrong), and document the current behaviour honestly.

## Final answer
- the pages you wrote, with one line each on what they cover;
- the snippets you compile-checked;
- any Navree bugs or rough edges you found;
- anything you couldn't document, and why.
