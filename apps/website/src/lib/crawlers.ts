/**
 * The crawlers of AI search and assistants, named in `robots.txt` so that
 * anyone reading it sees they are welcome. Each one is also covered by the
 * `*` rule; naming them keeps the permission explicit if the `*` rule ever
 * changes. `scripts/check-seo.ts` reads this list and looks for every name
 * in the built `robots.txt`.
 *
 * - search and answers: OAI-SearchBot (ChatGPT search), Claude-SearchBot,
 *   PerplexityBot;
 * - fetches a person asked for: ChatGPT-User, Claude-User, Perplexity-User;
 * - training and indexing: GPTBot, ClaudeBot, CCBot, and the Google-Extended
 *   and Applebot-Extended tokens, which only say whether Google and Apple may
 *   use pages for their AI models.
 *
 * Only erasable TypeScript and no path aliases: Node runs the check scripts
 * on this file directly.
 */
export const aiCrawlers = [
  "GPTBot",
  "OAI-SearchBot",
  "ChatGPT-User",
  "ClaudeBot",
  "Claude-SearchBot",
  "Claude-User",
  "PerplexityBot",
  "Perplexity-User",
  "Google-Extended",
  "Applebot-Extended",
  "CCBot",
] as const;
