/**
 * The selection diagrams of the UI docs: "which one should I use?". Each is a
 * question and the options that answer it, with the case each one fits.
 * `<UiChoose name="overlay" current="Dialog" />` draws one and lights the
 * page's own component.
 *
 * Plain data, so `src/lib/agent-markdown.ts` can turn a diagram into a list.
 * Keep `question` under 24 characters and each `when` under 34: they are set
 * in SVG at 1:1 and don't wrap.
 */

export type Choice = { name: string; when: string; href: string; mono?: boolean };
export type Choose = { title: string; question: string; options: Choice[] };

const c = (name: string, when: string, href: string, mono = true): Choice => ({ name, when, href, mono });

export const choices = {
  "choosing-a-ui": {
    title: "Choosing a UI",
    question: "Who draws your screens?",
    options: [
      c("Navree UI", "Rust pages, rendered on the server", "/docs/ui/overview/", false),
      c("Next.js (React)", "A React app beside the API", "/docs/frontend/nextjs/", false),
      c("Nuxt (Vue)", "A Vue app beside the API", "/docs/frontend/nuxt/", false),
    ],
  },
  "two-ways": {
    title: "Which way in?",
    question: "What do you bring?",
    options: [
      c("Bring your product", "Old code and data to rebuild", "/docs/getting-started/bring-your-product/", false),
      c("Bring your design", "A mock-up, an export, screenshots", "/docs/getting-started/bring-your-design/", false),
      c("Both", "An old product, Navree UI pages", "/docs/getting-started/bring-your-product/#how-the-two-work-together", false),
    ],
  },
  action: {
    title: "Which action?",
    question: "What does a click do?",
    options: [
      c("Button", "Runs an action or submits", "/docs/components/button/"),
      c("Button::href", "Goes to another page", "/docs/components/button/#a-link-that-looks-like-a-button"),
      c("Toggle", "Turns one thing on or off", "/docs/components/toggle/"),
      c("ButtonGroup", "One of a few related actions", "/docs/components/button-group/"),
    ],
  },
  "on-off": {
    title: "Which on and off control?",
    question: "When does it apply?",
    options: [
      c("Switch", "A setting, read as on or off", "/docs/components/switch/"),
      c("Checkbox", "Agreeing, or one of a list", "/docs/components/checkbox/"),
      c("Toggle", "A pressed button in a toolbar", "/docs/components/toggle/"),
    ],
  },
  "pick-one": {
    title: "Which choice control?",
    question: "How many choices?",
    options: [
      c("RadioGroup", "Two to five, all worth seeing", "/docs/components/radio-group/"),
      c("NativeSelect", "Six or more, or little room", "/docs/components/native-select/"),
      c("Checkbox", "Any number of them, not one", "/docs/components/checkbox/"),
    ],
  },
  text: {
    title: "Which text input?",
    question: "What is typed?",
    options: [
      c("Input", "One line: a name, a date", "/docs/components/input/"),
      c("Textarea", "Several lines: notes, a message", "/docs/components/textarea/"),
      c("InputGroup", "One line with a unit or an icon", "/docs/components/input-group/"),
    ],
  },
  field: {
    title: "How to label a control",
    question: "What surrounds it?",
    options: [
      c("Field", "Label, help and error, tied", "/docs/components/field/"),
      c("FieldSet", "Several fields under one legend", "/docs/components/field/#a-field-set"),
      c("FormView", "A whole form from a struct", "/docs/ui/forms/"),
    ],
  },
  surface: {
    title: "Which surface?",
    question: "What does it hold?",
    options: [
      c("Card", "A block of related content", "/docs/components/card/"),
      c("Item", "One row: a person, a setting", "/docs/components/item/"),
      c("Table", "Rows that share columns", "/docs/components/table/"),
    ],
  },
  label: {
    title: "Which small label?",
    question: "What does it name?",
    options: [
      c("Badge", "A status or a category", "/docs/components/badge/"),
      c("Kbd", "A key to press", "/docs/components/kbd/"),
      c("Avatar", "A person", "/docs/components/avatar/"),
    ],
  },
  message: {
    title: "Which message?",
    question: "What is there to say?",
    options: [
      c("Alert", "Something about this page", "/docs/components/alert/"),
      c("Empty", "A list has nothing in it", "/docs/components/empty/"),
      c("Reply::toast", "An action just finished", "/docs/ui/commands-and-ops/#toasts"),
      c("AlertDialog", "It must be answered first", "/docs/components/alert-dialog/"),
    ],
  },
  loading: {
    title: "Which loading state?",
    question: "What do you know?",
    options: [
      c("Skeleton", "The shape of what is coming", "/docs/components/skeleton/"),
      c("Progress", "How far along it is", "/docs/components/progress/"),
      c("Spinner", "Only that it is working", "/docs/components/spinner/"),
    ],
  },
  overlay: {
    title: "Which overlay?",
    question: "What does it hold?",
    options: [
      c("Dialog", "A short task: a form, a choice", "/docs/components/dialog/"),
      c("AlertDialog", "A question that must be answered", "/docs/components/alert-dialog/"),
      c("Sheet", "A long panel beside the page", "/docs/components/sheet/"),
      c("Drawer", "A task on a phone, from below", "/docs/components/drawer/"),
      c("Popover", "A few controls by their button", "/docs/components/popover/"),
    ],
  },
  hint: {
    title: "Which hint?",
    question: "What does it show?",
    options: [
      c("Tooltip", "A few words on a control", "/docs/components/tooltip/"),
      c("HoverCard", "A preview of a link's target", "/docs/components/hover-card/"),
      c("Popover", "Something to click or type in", "/docs/components/popover/"),
    ],
  },
  disclosure: {
    title: "Which disclosure?",
    question: "How much is hidden?",
    options: [
      c("Accordion", "Several sections, one by one", "/docs/components/accordion/"),
      c("Collapsible", "One section", "/docs/components/collapsible/"),
      c("ScrollArea", "Nothing: it scrolls in place", "/docs/components/scroll-area/"),
    ],
  },
  divider: {
    title: "Which layout piece?",
    question: "What do you need?",
    options: [
      c("Stack, Row, Grid", "Space between things", "/docs/ui/layout/"),
      c("Separator", "A line between things", "/docs/components/separator/"),
      c("AspectRatio", "A shape held as width changes", "/docs/components/aspect-ratio/"),
      c("Direction", "Right-to-left text in a part", "/docs/components/direction/"),
    ],
  },
  wayfinding: {
    title: "Which navigation?",
    question: "Where is the reader?",
    options: [
      c("Shell", "In the app: the sidebar", "/docs/ui/app-shell/"),
      c("Breadcrumb", "Deep in a hierarchy", "/docs/components/breadcrumb/"),
      c("Pagination", "On one page of a long list", "/docs/components/pagination/"),
    ],
  },
  data: {
    title: "Which table?",
    question: "Where do the rows live?",
    options: [
      c("Table", "A few rows you have in hand", "/docs/components/table/"),
      c("DataTable", "A query: sort, filter, select", "/docs/ui/data-table/"),
      c("Stat", "One figure that matters", "/docs/components/stat/"),
    ],
  },
  conversation: {
    title: "Which piece of a conversation?",
    question: "What is it?",
    options: [
      c("Message", "Who said it, and when", "/docs/components/message/"),
      c("Bubble", "What was said", "/docs/components/message/#bubbles"),
      c("Marker", "A date or an event between", "/docs/components/message/#markers"),
      c("Attachment", "A file that was shared", "/docs/components/attachment/"),
    ],
  },
  text_style: {
    title: "Which text?",
    question: "What is the words' job?",
    options: [
      c("Heading", "Names a page or a section", "/docs/components/typography/#headings"),
      c("Text", "Says something", "/docs/components/typography/#text"),
      c("Text::code", "A name from code", "/docs/components/typography/#text"),
    ],
  },
  update: {
    title: "How a page changes",
    question: "What starts the change?",
    options: [
      c("Cmd", "A click, with no request", "/docs/ui/commands-and-ops/#commands"),
      c("Reply", "A form or an action posted", "/docs/ui/commands-and-ops/#replies-and-ops"),
      c("Region", "Time, or data changing", "/docs/ui/regions/"),
    ],
  },
} satisfies Record<string, Choose>;

export type ChooseName = keyof typeof choices;
