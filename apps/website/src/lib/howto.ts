import GithubSlugger from "github-slugger";

/**
 * `HowTo` structured data for the docs pages that are a list of steps. The
 * steps are headings of the page: each entry names the heading it follows,
 * and `howToFor` fails when a heading is renamed or removed, so the data
 * can't point at a section that is gone. The anchors come from the page's
 * own headings, slugged the way the page slugs them.
 *
 * The texts are short instructions; the page says the rest.
 */
export type HowToSpec = {
  name: string;
  description: string;
  steps: { heading: string; text: string }[];
};

export const howTos: Record<string, HowToSpec> = {
  "getting-started/installation": {
    name: "How to install Navree and create an app",
    description: "Install Rust and the navree CLI, create an app with navree new, start its database and run it with navree dev.",
    steps: [
      {
        heading: "Installing the CLI",
        text: "Install Rust 1.95 or newer, then install the navree command-line tool from the Git repository: cargo install --git https://github.com/NavreeHQ/navree navree-cli.",
      },
      { heading: "Creating an application", text: "Run navree new blog to create an app in a new directory." },
      {
        heading: "Local services with Dock",
        text: "From the app's directory, run navree dock up to start the app's TimescaleDB database in a container.",
      },
      {
        heading: "Running the application",
        text: "Start the development server with navree dev. navree new has already put an application key in .env. The API listens on http://127.0.0.1:8080.",
      },
    ],
  },
  "getting-started/your-first-app": {
    name: "How to build a task-list API with Navree",
    description:
      "Build a small task-list API with a model, a migration, validation, JWT authentication, access rules and feature tests, from navree new to green tests.",
    steps: [
      {
        heading: "Create the app",
        text: "Run navree new planner --ui none, then navree dock up. The app's .env already has an application key.",
      },
      { heading: "Generate the tasks module", text: "Run navree make:module tasks --crud to generate a module with create, read, update and delete routes." },
      { heading: "Define the table", text: "Edit the generated migration so the tasks table has title and done columns." },
      { heading: "Update the model", text: "Change the model's fields to match the table; the generated routes return the model as it is." },
      { heading: "Validate the input", text: "Describe the request bodies as structs with #[derive(Validate)] rules; the Valid extractor checks them before the handler runs." },
      { heading: "Write the service", text: "Change the service's two methods that set columns, so a task is created and updated with its title and done flag." },
      { heading: "Decide who may do what", text: "Run navree make:policy tasks task and write the rules that let a user act on their own tasks." },
      { heading: "Try it", text: "Start the app with navree dev, sign up with the register route, and call the tasks routes with the bearer token." },
      { heading: "Write a feature test", text: "Create tests/tasks.rs to test the whole app the way a client uses it: real sign-ups, real tokens and the real abilities." },
      { heading: "Draw the tasks with Navree UI", text: "Add SessionModule, UiModule and AuthPages, write a TaskPages controller that returns a View, and open /tasks after signing in." },
    ],
  },
};

/** Remove an MDX heading's markup: `` `code` `` and links become their text. */
const headingText = (raw: string) =>
  raw
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/<[^>]+>/g, "")
    .replace(/[`*_]/g, "")
    .trim();

/**
 * The `HowTo` for a docs page, or null when the page has none. `source` is
 * the page's MDX. Throws when a step names a heading the page doesn't have.
 */
export function howToFor(slug: string, source: string): { name: string; description: string; steps: { name: string; text: string; anchor: string }[] } | null {
  const spec = howTos[slug];
  if (!spec) return null;

  // The anchors of the page's ## headings, outside code blocks, as rehype-slug makes them.
  const slugger = new GithubSlugger();
  const anchors = new Map<string, string>();
  let fence: string | null = null;
  for (const line of source.replace(/\r\n?/g, "\n").split("\n")) {
    const marker = /^ {0,3}(`{3,}|~{3,})/.exec(line)?.[1];
    if (marker) {
      if (fence === null) fence = marker;
      else if (marker[0] === fence[0] && marker.length >= fence.length) fence = null;
      continue;
    }
    if (fence !== null) continue;
    const heading = /^(#{2,6}) +(.+?)\s*#*\s*$/.exec(line);
    if (!heading) continue;
    const text = headingText(heading[2]);
    const id = slugger.slug(text);
    if (heading[1] === "##" && !anchors.has(text)) anchors.set(text, id);
  }

  return {
    name: spec.name,
    description: spec.description,
    steps: spec.steps.map((step) => {
      const anchor = anchors.get(step.heading);
      if (!anchor) throw new Error(`howto: ${slug} has no "## ${step.heading}" heading (src/lib/howto.ts)`);
      return { name: step.heading, text: step.text, anchor };
    }),
  };
}
