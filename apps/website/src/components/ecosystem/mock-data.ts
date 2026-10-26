/**
 * Data for the product mocks on /ecosystem/ and the homepage: the example
 * CRM (deals, companies, contacts, invoices) on crm.example.com, with the
 * routes, commands, config keys and outputs the tool docs use.
 */

// Lighthouse ------------------------------------------------------------------

export type IssueKind = "exception" | "slow-query" | "slow-request" | "failed-job" | "outgoing" | "regression";

export const issueKinds: Record<IssueKind, { label: string; tone: "danger" | "amber" }> = {
  exception: { label: "Exception", tone: "danger" },
  regression: { label: "Regression", tone: "danger" },
  "failed-job": { label: "Failed job", tone: "danger" },
  "slow-query": { label: "Slow query", tone: "amber" },
  "slow-request": { label: "Slow request", tone: "amber" },
  outgoing: { label: "Outgoing", tone: "amber" },
};

export const lighthouseFigures = [
  { label: "Requests / min", value: "1,284", sub: "p50 22 ms", spark: [40, 44, 39, 52, 61, 58, 66, 72, 69, 81, 78, 84] },
  { label: "Error rate", value: "0.21%", sub: "4xx and 5xx" },
  { label: "p95 latency", value: "184 ms", sub: "all routes", spark: [120, 132, 128, 141, 150, 139, 162, 158, 171, 166, 180, 184] },
  { label: "Failed jobs", value: "3", sub: "last 24 hours" },
  { label: "Open issues", value: "7", sub: "2 new today" },
] as const;

export const lighthouseIssues: {
  kind: IssueKind;
  title: string;
  culprit: string;
  events: string;
  users: string;
  seen: string;
  spark: number[];
}[] = [
  {
    kind: "slow-query",
    title: "SELECT … FROM contacts WHERE id = ?  × 11",
    culprit: "GET /api/v1/deals/board",
    events: "342",
    users: "28",
    seen: "2 min",
    spark: [3, 4, 3, 6, 8, 7, 12, 14, 13, 18, 22, 25],
  },
  {
    kind: "exception",
    title: "cannot charge invoice: card declined",
    culprit: "POST /api/v1/invoices/{id}/charge",
    events: "12",
    users: "9",
    seen: "4 min",
    spark: [0, 1, 0, 0, 2, 1, 0, 3, 1, 2, 0, 2],
  },
  {
    kind: "regression",
    title: "panicked: stage probability out of range",
    culprit: "src/modules/deals/service.rs:214",
    events: "3",
    users: "2",
    seen: "18 min",
    spark: [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1],
  },
  {
    kind: "slow-request",
    title: "GET /api/v1/deals/board  1,412 ms",
    culprit: "DealsController::board",
    events: "96",
    users: "21",
    seen: "3 min",
    spark: [2, 3, 2, 4, 5, 4, 7, 8, 6, 9, 11, 10],
  },
  {
    kind: "failed-job",
    title: "contacts.export: timed out after 120 s",
    culprit: "job contacts.export · queue exports",
    events: "5",
    users: "3",
    seen: "1 h",
    spark: [0, 0, 1, 0, 0, 2, 0, 0, 1, 0, 1, 0],
  },
];

export type Occurrence = { id: string; kind: IssueKind; title: string; culprit: string };

/** The live feed: new occurrences arrive on top, in this order, forever. */
export const lighthouseFeed: Occurrence[] = [
  { id: "o1", kind: "exception", title: "cannot charge invoice: card declined", culprit: "POST /api/v1/invoices/{id}/charge" },
  { id: "o2", kind: "slow-query", title: "SELECT … FROM contacts WHERE id = ?  1,203 ms", culprit: "GET /api/v1/deals/board" },
  { id: "o3", kind: "failed-job", title: "contacts.export timed out after 120 s", culprit: "queue exports · attempt 3 of 3" },
  { id: "o4", kind: "slow-request", title: "GET /api/v1/deals/board  1,412 ms", culprit: "trace 4bf92f3577b34da6" },
  { id: "o5", kind: "regression", title: "stage probability out of range", culprit: "release 20261026-091512-3f2a9c1" },
  { id: "o6", kind: "outgoing", title: "GET api.exchangerate.example timed out", culprit: "HttpClient · after 2,000 ms" },
  { id: "o7", kind: "exception", title: "mail transport refused the connection", culprit: "job mail.send · queue mail" },
  { id: "o8", kind: "slow-query", title: "SELECT … FROM activities WHERE deal_id = ?  286 ms", culprit: "GET /api/v1/deals/{id}" },
  { id: "o9", kind: "slow-request", title: "POST /api/v1/contacts/import  3,870 ms", culprit: "ContactsController::import" },
  { id: "o10", kind: "exception", title: "unique violation on companies.domain", culprit: "POST /api/v1/companies" },
  { id: "o11", kind: "failed-job", title: "webhooks.deliver: endpoint answered 410 Gone", culprit: "queue default · attempt 8 of 8" },
  { id: "o12", kind: "outgoing", title: "POST payments.example/v1/charges answered 503", culprit: "HttpClient · after 412 ms" },
];

/** Requests (up) and exceptions (down) per half hour over 24 hours. */
export const lighthouseTimeline = {
  requests: [
    22, 18, 15, 12, 10, 9, 9, 10, 12, 15, 20, 28, 38, 49, 58, 64, 70, 74, 77, 79, 80, 82, 81, 79, 78, 80, 83, 85, 84, 82, 78, 76,
    74, 71, 66, 60, 54, 49, 45, 42, 40, 38, 36, 34, 31, 29, 27, 25,
  ],
  exceptions: [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 2, 1, 0, 1, 3, 1, 0, 2, 1, 0, 1, 0, 6, 9, 4, 2, 1, 0, 1, 0, 0, 2, 1, 0, 0, 1, 0, 0, 1, 0,
    0, 0, 1, 0,
  ],
};

// Fleet ------------------------------------------------------------------------

export type QueueState = { processes: number; pending: string; wait: string; clears: string };

/**
 * A newsletter queues 3,000 mails at 09:00: `mail` grows from one process to
 * six while `default` and `notifications` keep what they need, then gives
 * them back as it drains (the balancing example in the Fleet docs).
 */
export const fleetSteps: { time: string; jobsPerMin: string; maxWait: string; maxWaitQueue: string; queues: Record<"default" | "mail" | "notifications", QueueState> }[] = [
  {
    time: "08:59:52",
    jobsPerMin: "476",
    maxWait: "6 s",
    maxWaitQueue: "exports",
    queues: {
      default: { processes: 3, pending: "12", wait: "3 s", clears: "4 s" },
      mail: { processes: 1, pending: "0", wait: "0 s", clears: "now" },
      notifications: { processes: 1, pending: "0", wait: "0 s", clears: "now" },
    },
  },
  {
    time: "09:00:01",
    jobsPerMin: "512",
    maxWait: "9 s",
    maxWaitQueue: "mail",
    queues: {
      default: { processes: 3, pending: "10", wait: "2 s", clears: "3 s" },
      mail: { processes: 2, pending: "3,000", wait: "9 s", clears: "6 min" },
      notifications: { processes: 1, pending: "2", wait: "0 s", clears: "1 s" },
    },
  },
  {
    time: "09:00:04",
    jobsPerMin: "688",
    maxWait: "12 s",
    maxWaitQueue: "mail",
    queues: {
      default: { processes: 2, pending: "6", wait: "2 s", clears: "3 s" },
      mail: { processes: 3, pending: "2,912", wait: "12 s", clears: "4 min" },
      notifications: { processes: 1, pending: "0", wait: "0 s", clears: "now" },
    },
  },
  {
    time: "09:00:07",
    jobsPerMin: "904",
    maxWait: "13 s",
    maxWaitQueue: "mail",
    queues: {
      default: { processes: 2, pending: "4", wait: "1 s", clears: "2 s" },
      mail: { processes: 4, pending: "2,760", wait: "13 s", clears: "3 min" },
      notifications: { processes: 1, pending: "1", wait: "0 s", clears: "1 s" },
    },
  },
  {
    time: "09:00:10",
    jobsPerMin: "1,126",
    maxWait: "11 s",
    maxWaitQueue: "mail",
    queues: {
      default: { processes: 1, pending: "3", wait: "2 s", clears: "3 s" },
      mail: { processes: 5, pending: "2,540", wait: "11 s", clears: "2 min" },
      notifications: { processes: 1, pending: "0", wait: "0 s", clears: "now" },
    },
  },
  {
    time: "09:00:13",
    jobsPerMin: "1,318",
    maxWait: "8 s",
    maxWaitQueue: "mail",
    queues: {
      default: { processes: 1, pending: "2", wait: "1 s", clears: "2 s" },
      mail: { processes: 6, pending: "2,264", wait: "8 s", clears: "90 s" },
      notifications: { processes: 1, pending: "0", wait: "0 s", clears: "now" },
    },
  },
  {
    time: "09:01:40",
    jobsPerMin: "1,342",
    maxWait: "6 s",
    maxWaitQueue: "exports",
    queues: {
      default: { processes: 1, pending: "1", wait: "0 s", clears: "1 s" },
      mail: { processes: 6, pending: "310", wait: "2 s", clears: "14 s" },
      notifications: { processes: 1, pending: "0", wait: "0 s", clears: "now" },
    },
  },
  {
    time: "09:02:05",
    jobsPerMin: "702",
    maxWait: "6 s",
    maxWaitQueue: "exports",
    queues: {
      default: { processes: 2, pending: "8", wait: "1 s", clears: "3 s" },
      mail: { processes: 3, pending: "0", wait: "0 s", clears: "now" },
      notifications: { processes: 1, pending: "0", wait: "0 s", clears: "now" },
    },
  },
];

/** Mail jobs per minute over the last 30 minutes, ending with the newsletter. */
export const fleetMailPerMinute = [
  31, 28, 35, 30, 33, 29, 36, 34, 31, 30, 38, 33, 29, 35, 32, 30, 34, 37, 31, 33, 36, 30, 34, 32, 35, 640, 980, 1240, 1310, 1290,
];

export const fleetFailedJobs = [
  { type: "contacts.export", queue: "exports", attempts: "3/3", error: "timed out after 120 s", at: "08:41" },
  { type: "mail.send", queue: "mail", attempts: "5/5", error: "connection refused: smtp.internal:587", at: "08:12" },
  { type: "webhooks.deliver", queue: "default", attempts: "8/8", error: "endpoint answered 410 Gone", at: "07:55" },
];

// Harbor -----------------------------------------------------------------------

/** `navree deploy`, as the Harbor docs print it. */
export const harborDeploy: { step: string; server?: string; detail: string; time?: string }[] = [
  { step: "build", detail: "crm-api for x86_64-unknown-linux-gnu in a container", time: "1m 42s" },
  { step: "upload", server: "app-1", detail: "releases/20261026-091512-3f2a9c1 (14.2 MB)", time: "3s" },
  { step: "env", server: "app-1", detail: "4 settings, 3 secrets" },
  { step: "migrate", server: "app-1", detail: "2 migrations ran", time: "1s" },
  { step: "start", server: "app-1", detail: "green on 127.0.0.1:8082" },
  { step: "health", server: "app-1", detail: "GET /health/ready 200 after 0.4 s" },
  { step: "switch", server: "app-1", detail: "Caddy -> green, current -> 20261026-091512-3f2a9c1" },
  { step: "drain", server: "app-1", detail: "blue stopped after its requests finished" },
  { step: "restart", server: "app-1", detail: "worker x2, scheduler" },
  { step: "prune", server: "app-1", detail: "1 old release removed" },
];

export const harborReleases = [
  { id: "20261026-091512-3f2a9c1", commit: "feat: stage probabilities" },
  { id: "20261025-163003-9b17e44", commit: "fix: deal board ordering" },
  { id: "20261024-110741-52ad0f3", commit: "chore: dependencies" },
];

// Helm -------------------------------------------------------------------------

export type Stage = "Lead" | "Qualified" | "Proposal" | "Negotiation" | "Won" | "Lost";

export const helmDeals: {
  title: string;
  amount: string;
  stage: Stage;
  company: string;
  owner: string;
  close: string;
  probability: string;
  /** Ticked in the bulk-action demo. */
  pick?: boolean;
}[] = [
  { title: "Migration from legacy CRM", amount: "$84,000", stage: "Negotiation", company: "Northwind Logistics", owner: "Priya Shah", close: "Nov 14", probability: "75%", pick: true },
  { title: "Three-year enterprise agreement", amount: "$212,500", stage: "Negotiation", company: "Atlas Ridge Capital", owner: "Priya Shah", close: "Nov 21", probability: "75%", pick: true },
  { title: "Executive dashboard suite", amount: "$36,200", stage: "Proposal", company: "Brightwater Analytics", owner: "Tom Becker", close: "Nov 28", probability: "50%" },
  { title: "Forecasting add-on", amount: "$18,900", stage: "Negotiation", company: "Halcyon Health", owner: "Priya Shah", close: "Dec 2", probability: "75%", pick: true },
  { title: "Compliance module", amount: "$47,000", stage: "Proposal", company: "Quillstone Legal", owner: "Tom Becker", close: "Dec 9", probability: "50%" },
  { title: "Multi-site deployment", amount: "$126,000", stage: "Qualified", company: "Meridian Freight", owner: "Marcus Reyes", close: "Jan 6", probability: "25%" },
];

export const helmStages: { stage: Stage; deals: number }[] = [
  { stage: "Lead", deals: 6 },
  { stage: "Qualified", deals: 6 },
  { stage: "Proposal", deals: 5 },
  { stage: "Negotiation", deals: 5 },
  { stage: "Won", deals: 5 },
  { stage: "Lost", deals: 3 },
];

export const helmNewDeals = [2, 3, 1, 4, 3, 2, 5, 3, 4, 6, 4, 3, 5, 7, 4, 6, 5, 8, 6, 7, 5, 9, 7, 8, 6, 9, 10, 8, 11, 12];

// Logbook ----------------------------------------------------------------------

export type LogKind = "request" | "access" | "query" | "model" | "event" | "job" | "mail" | "cache" | "log" | "response";

export const logbookBatches: {
  label: string;
  status: string;
  duration: string;
  counts: string;
  warn?: string;
  selected?: boolean;
  /** Shown before the label of a batch that isn't a request: `job` or `task`. */
  prefix?: string;
}[] = [
  { label: "POST /api/v1/deals", status: "201", duration: "24 ms", counts: "6 q · 2 ev · 1 job · 1 mail", selected: true },
  { label: "deals.notify-owner", status: "done", duration: "112 ms", counts: "3 q · 1 mail", prefix: "job" },
  { label: "GET /api/v1/deals", status: "200", duration: "64 ms", counts: "23 q", warn: "duplicate queries" },
  { label: "GET /api/v1/deals/board", status: "200", duration: "31 ms", counts: "4 q · 1 cache" },
  { label: "POST /api/auth/refresh", status: "200", duration: "9 ms", counts: "2 q" },
  { label: "GET /api/v1/companies/{id}", status: "404", duration: "6 ms", counts: "1 q" },
  { label: "GET /api/v1/contacts?q=ada", status: "200", duration: "18 ms", counts: "2 q · 1 cache" },
  { label: "crm:task-reminders", status: "done", duration: "240 ms", counts: "5 q · 3 mail", prefix: "task" },
  { label: "PATCH /api/v1/deals/{id}", status: "200", duration: "21 ms", counts: "4 q · 1 ev" },
];

/** The timeline of `POST /api/v1/deals`: start and duration in ms of 24. */
export const logbookTimeline: { kind: LogKind; text: string; start: number; duration: number }[] = [
  { kind: "access", text: "can create Deal: allowed by role rep", start: 0.2, duration: 0.3 },
  { kind: "query", text: "BEGIN", start: 0.6, duration: 0.4 },
  { kind: "query", text: "SELECT … FROM companies WHERE id = ?", start: 1.1, duration: 1.9 },
  { kind: "query", text: "SELECT max(position) FROM deals WHERE stage = ?", start: 3.2, duration: 1.3 },
  { kind: "query", text: "INSERT INTO deals (id, title, amount_cents, …)", start: 4.7, duration: 2.6 },
  { kind: "model", text: "deal:0199c1d2 created", start: 7.4, duration: 0.3 },
  { kind: "event", text: "deal.created → 2 listeners", start: 7.9, duration: 3.1 },
  { kind: "query", text: "INSERT INTO audit_log (action, actor_id, …)", start: 8.4, duration: 1.2 },
  { kind: "job", text: "deals.notify-owner queued on default", start: 11.2, duration: 0.9 },
  { kind: "mail", text: "“New deal: Migration from legacy CRM” queued", start: 12.3, duration: 0.8 },
  { kind: "query", text: "COMMIT", start: 13.3, duration: 2.4 },
  { kind: "cache", text: "forget dashboard:pipeline:acme", start: 15.9, duration: 0.3 },
  { kind: "log", text: "INFO deal created deal_id=0199c1d2…", start: 16.4, duration: 0.2 },
  { kind: "response", text: "201 Created · 1.3 KB", start: 17.1, duration: 6.9 },
];

// Navigator --------------------------------------------------------------------

export const navigatorTools = [
  "app_info",
  "list_routes",
  "module_graph",
  "list_providers",
  "list_commands",
  "describe_schema",
  "query_database",
  "read_logs",
  "last_error",
  "read_config",
  "search_docs",
  "read_doc",
];

/** The exchange from the Navigator docs. */
export const navigatorSession: (
  | { type: "prompt"; text: string }
  | { type: "call"; tool: string; args?: string; result: string[] }
  | { type: "answer"; text: string }
)[] = [
  { type: "prompt", text: "The deals page is empty since I pulled main, and the API returns 500. Fix it." },
  {
    type: "call",
    tool: "last_error",
    result: ["500 GET /api/v1/deals  request_id=0199c4b1-…", "error: no such column: deals.expected_close_date"],
  },
  {
    type: "call",
    tool: "describe_schema",
    args: 'table: "deals"',
    result: ["deals: id, title, amount_cents, currency, stage, position, …", "pending migrations: crm/m20261026_000004_add_expected_close_date"],
  },
  { type: "call", tool: "list_commands", result: ["… migrate   Run every pending migration …"] },
  {
    type: "answer",
    text: "The code you pulled reads deals.expected_close_date, but the migration that adds it hasn’t run on your database. Run `navree migrate` and reload the page.",
  },
];
