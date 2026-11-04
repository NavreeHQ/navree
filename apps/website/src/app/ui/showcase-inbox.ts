/**
 * The support inbox's entry in the example apps: on `/ui/` ("Build anything") and on
 * the Showcase. Each view is a ui-docs example drawn from the app's own views.
 */

/** Where the inbox sits in "Build anything" on `/ui/`. */
export const inboxShowcase = {
  id: "inbox",
  app: "inbox-demo",
  title: "A support inbox that stays fast at 100,000 conversations",
  lead: "Three panes you resize, a thread with notes and files, the customer beside it, and new messages arriving while you type. Every list loads as it is read, so the first screen costs one page whatever the desk holds.",
  points: [
    "A [SplitPane](/docs/components/split-pane/) dragged or moved with the keys, its sizes kept for each agent on the server",
    "[LazyList](/docs/components/lazy-list/) pages by cursor: fifty conversations, thirty messages, older ones above",
    "Live over server-sent events: the list and the open thread merge what changed, and what you type stays",
    "Assign through a [Combobox](/docs/components/combobox/) searched on the server, snooze with a [DatePicker](/docs/components/date-picker/), act from the [command palette](/docs/components/command/)",
    "Keys `j`, `k`, `e`, `r` and `/`, and every action as a link or a form with JavaScript off",
  ],
  views: [
    {
      example: "build-anything/inbox-panes",
      label: "Inbox",
      title: "The support inbox: the conversations, an open conversation with an internal note and a file, and the customer's plan, history and tags, rendered by navree-ui",
      caption:
        "Replayed in your browser: a new conversation joins the top and the customer writes again. In the app, the server streams them over SSE.",
      file: "src/views/inbox.rs",
    },
    {
      example: "build-anything/inbox-thread",
      label: "100 messages",
      title: "A conversation of 100 messages opened at its newest thirty, with a link to earlier messages, rendered by navree-ui",
      caption:
        "A long conversation opens at its newest thirty messages; older ones load as you scroll up, and you keep your place. Replayed in your browser: in the app each page comes from the server by cursor.",
      file: "src/views/thread.rs",
    },
    {
      example: "build-anything/inbox-phone",
      label: "On a phone",
      title: "The inbox on a phone: one pane at a time, with a link back to the conversations, rendered by navree-ui",
      caption: "On a phone the panes show one at a time, with links between them.",
      file: "src/views/inbox.rs",
    },
    {
      example: "build-anything/inbox-palette",
      label: "Command palette",
      title: "The command palette: actions on the open conversation, saved replies and places to go, rendered by navree-ui",
      caption: "Ctrl+K: assign, close, snooze, change the priority, or drop a saved reply into what you are writing.",
      file: "src/views/inbox.rs",
      interactive: true,
    },
  ],
};

/** The inbox's card on `/showcase/`. */
export const inboxApp = {
  slug: "inbox",
  app: "inbox-demo",
  name: "A support inbox",
  summary:
    "Conversations, a thread and the customer in three panes you resize, live over server-sent events, proven at 100,000 conversations.",
  facts: [
    { label: "Data", value: "SQLite, 100,000 conversations" },
    { label: "Messages", value: "About 5.5 million" },
    { label: "Lists", value: "Lazy, by cursor" },
    { label: "Live", value: "Server-sent events" },
  ],
  preview: "build-anything/inbox-panes",
  previewTitle: "The support inbox in three panes, rendered by navree-ui",
};
