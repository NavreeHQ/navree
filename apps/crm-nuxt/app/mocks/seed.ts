/**
 * Demo data for the mock API, mirroring crm-api's `crm-demo` seeder:
 * four accounts (password `password123`), ~20 companies, ~50 contacts,
 * ~30 deals across stages and ~60 activities. Generated deterministically;
 * dates are relative to "now" so overdue/today/upcoming buckets always exist.
 */
import type { ActivityKind, CompanySize, ContactStatus, DealStage, Role } from "@/lib/api/types";

export interface UserRow {
  id: string;
  name: string;
  email: string;
  password: string;
  role: Role;
  email_verified_at: string | null;
  created_at: string;
}

export interface CompanyRow {
  id: string;
  name: string;
  domain: string | null;
  industry: string | null;
  size: CompanySize | null;
  phone: string | null;
  website: string | null;
  city: string | null;
  country: string | null;
  owner_id: string;
  created_at: string;
  updated_at: string;
  deleted_at: string | null;
}

export interface ContactRow {
  id: string;
  first_name: string;
  last_name: string;
  email: string | null;
  phone: string | null;
  title: string | null;
  status: ContactStatus;
  company_id: string | null;
  owner_id: string;
  created_at: string;
  updated_at: string;
  deleted_at: string | null;
}

export interface DealRow {
  id: string;
  title: string;
  amount_cents: number;
  currency: string;
  stage: DealStage;
  position: number;
  expected_close_date: string | null;
  closed_at: string | null;
  lost_reason: string | null;
  company_id: string;
  contact_id: string | null;
  owner_id: string;
  created_at: string;
  updated_at: string;
  deleted_at: string | null;
}

export interface ActivityRow {
  id: string;
  kind: ActivityKind;
  subject: string;
  body: string | null;
  due_at: string | null;
  completed_at: string | null;
  subject_type: "company" | "contact" | "deal";
  subject_id: string;
  owner_id: string;
  assignee_id: string | null;
  created_at: string;
}

export interface NotificationRow {
  id: string;
  user_id: string;
  type: string;
  data: Record<string, unknown>;
  read_at: string | null;
  created_at: string;
}

export interface AuditRow {
  id: string;
  kind: string;
  action: string;
  actor_id: string | null;
  subject_type: string | null;
  subject_id: string | null;
  old_values: Record<string, unknown> | null;
  new_values: Record<string, unknown> | null;
  request_id: string | null;
  created_at: string;
}

/**
 * The audit `kind` and `action` crm-api records (see `app/features/audit/actions.ts`):
 * data changes are `<entity>.created|updated|deleted` (completing a task is an
 * update, deleting a company or contact sets `deleted_at`), sign-ins `auth.login` /
 * `auth.logout` without a subject, role changes `access.role_assigned` /
 * `access.user_roles_synced` about a `user`.
 */
export function auditAction(
  kind: string,
  action: string,
  subjectType: string | null,
): { kind: string; action: string; subject: boolean } {
  if (kind === "auth") return { kind: "auth", action: `auth.${action}`, subject: false };
  if (kind === "access") return { kind: "access", action: `access.${action}`, subject: true };
  const verb = action === "completed" ? "updated" : action;
  return { kind: "data", action: `${(subjectType ?? "record").toLowerCase()}.${verb}`, subject: true };
}

export interface MockDatabase {
  users: UserRow[];
  companies: CompanyRow[];
  contacts: ContactRow[];
  deals: DealRow[];
  activities: ActivityRow[];
  notifications: NotificationRow[];
  audit: AuditRow[];
}

export const DEMO_PASSWORD = "password123";

/** mulberry32: small deterministic PRNG. */
function prng(seed: number) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** UUID v7-shaped id: 48-bit timestamp + random bits. */
export function makeId(time: number, rand: () => number = Math.random): string {
  const hex = (n: number, len: number) => Math.floor(n).toString(16).padStart(len, "0").slice(-len);
  const ts = hex(time, 12);
  const r = () => hex(rand() * 0x10000, 4);
  const variant = (8 + Math.floor(rand() * 4)).toString(16);
  return `${ts.slice(0, 8)}-${ts.slice(8, 12)}-7${r().slice(1)}-${variant}${r().slice(1)}-${r()}${r()}${r()}`;
}

const DAY = 86_400_000;

/** `items[index % items.length]`: seed lists are never empty, so this always returns an item. */
function at<T>(items: readonly T[], index: number): T {
  const item = items[index % items.length];
  if (item === undefined) throw new Error("mock seed: empty list");
  return item;
}

export function createSeed(now: Date = new Date()): MockDatabase {
  const rand = prng(20261025);
  const pick = <T>(items: readonly T[]): T => at(items, Math.floor(rand() * items.length));
  const int = (min: number, max: number) => min + Math.floor(rand() * (max - min + 1));
  const nowMs = now.getTime();
  const iso = (ms: number) => new Date(ms).toISOString();
  const daysAgo = (d: number, hour = 10) => {
    const date = new Date(nowMs - d * DAY);
    date.setHours(hour, int(0, 59), 0, 0);
    return Math.min(date.getTime(), nowMs - 60_000);
  };
  const idAt = (ms: number) => makeId(ms, rand);
  // Day and month boundaries are UTC, like the API.
  const startOfToday = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate());
  const dateOnly = (ms: number) => {
    const d = new Date(ms);
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  };

  // ---- Users -----------------------------------------------------------------
  const userSpec: [string, string, Role][] = [
    ["Daljit Kaur", "admin@navree.test", "admin"],
    ["Manpreet Gill", "manager@navree.test", "manager"],
    ["Priya Verma", "rep1@navree.test", "rep"],
    ["Tarun Bhatia", "rep2@navree.test", "rep"],
  ];
  const users: UserRow[] = userSpec.map(([name, email, role], i) => {
    const created = daysAgo(400 - i * 20);
    return {
      id: idAt(created),
      name,
      email,
      password: DEMO_PASSWORD,
      role,
      email_verified_at: iso(created + 3600_000),
      created_at: iso(created),
    };
  });
  const [admin, manager, rep1, rep2] = [0, 1, 2, 3].map((i) => at(users, i)) as [UserRow, UserRow, UserRow, UserRow];
  const sellers = [manager, rep1, rep1, rep2, rep2];

  // ---- Companies -------------------------------------------------------------
  const companySpec: [string, string, string, CompanySize, string, string][] = [
    ["Doaba Logistics", "doabalogistics.in", "Logistics", "201-1000", "Jalandhar", "India"],
    ["Shivalik Health", "shivalikhealth.in", "Healthcare", "1000+", "Chandigarh", "India"],
    ["Beas Analytics", "beasanalytics.in", "Software", "51-200", "Mohali", "India"],
    ["Chenab Manufacturing", "chenabmfg.in", "Manufacturing", "201-1000", "Ludhiana", "India"],
    ["Phulkari Outfitters", "phulkarioutfitters.in", "Retail", "51-200", "Amritsar", "India"],
    ["Jhelum Freight Lines", "jhelumfreight.in", "Logistics", "1000+", "Delhi", "India"],
    ["Lakshya Learning Co.", "lakshyalearning.in", "Education", "11-50", "Jaipur", "India"],
    ["Majha Capital Partners", "majhacapital.in", "Financial services", "11-50", "Mumbai", "India"],
    ["Sarson Foods", "sarsonfoods.in", "Food & beverage", "201-1000", "Patiala", "India"],
    ["Sirhind Legal Associates", "sirhindlegal.in", "Legal", "51-200", "Gurugram", "India"],
    ["Vasant Media", "vasantmedia.in", "Media", "11-50", "Noida", "India"],
    ["Indrapuri Energy", "indrapurienergy.in", "Energy", "1000+", "Delhi", "India"],
    ["Chandni Cloudworks", "chandnicloud.in", "Software", "201-1000", "Pune", "India"],
    ["Teej Hospitality", "teejhospitality.in", "Hospitality", "1000+", "Jaipur", "India"],
    ["Gehun Agritech", "gehunagritech.in", "Agriculture", "51-200", "Bathinda", "India"],
    ["Hariyali Realty", "hariyalirealty.in", "Real estate", "11-50", "Dehradun", "India"],
    ["Kikar Aerospace", "kikaraero.in", "Aerospace", "1000+", "Bengaluru", "India"],
    ["Malwa Health Partners", "malwahealth.in", "Healthcare", "201-1000", "Lucknow", "India"],
    ["Pipal Software", "pipalsoft.in", "Software", "1-10", "Chandigarh", "India"],
    ["Sutlej Biolabs", "sutlejbio.in", "Biotech", "51-200", "Mohali", "India"],
    ["Dhanak Design Studio", "dhanakstudio.in", "Design", "1-10", "Jaipur", "India"],
  ];
  const companies: CompanyRow[] = companySpec.map(([name, domain, industry, size, city, country], i) => {
    const created = daysAgo(300 - i * 12);
    const owner = i < 6 ? at([rep1, rep2, manager], i) : pick(sellers);
    return {
      id: idAt(created),
      name,
      domain,
      industry,
      size,
      phone: `+91 ${int(70, 99)}${int(100, 999)} ${String(int(0, 99999)).padStart(5, "0")}`,
      website: `https://${domain}`,
      city,
      country,
      owner_id: owner.id,
      created_at: iso(created),
      updated_at: iso(created + int(1, 30) * DAY),
      deleted_at: null,
    };
  });

  // ---- Contacts --------------------------------------------------------------
  const firstNames = [
    "Harleen",
    "Arjun",
    "Jaspreet",
    "Rohit",
    "Simran",
    "Amandeep",
    "Neha",
    "Vikram",
    "Ananya",
    "Navdeep",
    "Kavya",
    "Aditya",
    "Ravneet",
    "Ishaan",
    "Pooja",
    "Siddharth",
    "Meera",
    "Kulwinder",
    "Rajveer",
    "Sandeep",
    "Gurleen",
    "Manish",
    "Tanvi",
    "Harpreet",
    "Nisha",
    "Karan",
    "Divya",
    "Lovepreet",
    "Anjali",
    "Mohit",
  ];
  const lastNames = [
    "Dhillon",
    "Sharma",
    "Gill",
    "Verma",
    "Sandhu",
    "Malhotra",
    "Grewal",
    "Kapoor",
    "Sidhu",
    "Arora",
    "Brar",
    "Khanna",
    "Bains",
    "Mehra",
    "Chopra",
    "Bhatia",
    "Gupta",
    "Agarwal",
    "Joshi",
    "Randhawa",
    "Walia",
    "Saini",
    "Kohli",
    "Ahluwalia",
    "Tiwana",
    "Mann",
    "Chahal",
    "Bajwa",
    "Sethi",
    "Anand",
  ];
  const titles = [
    "VP of Operations",
    "Head of Procurement",
    "Chief Technology Officer",
    "Finance Director",
    "Operations Manager",
    "IT Director",
    "Chief Executive Officer",
    "Head of Partnerships",
    "Product Lead",
    "Supply Chain Manager",
    "Office Manager",
    "General Counsel",
    "Marketing Director",
  ];
  const statuses: ContactStatus[] = ["lead", "lead", "prospect", "prospect", "customer", "customer", "churned"];
  const contacts: ContactRow[] = [];
  for (let i = 0; i < 52; i++) {
    const company = i < 42 ? at(companies, i) : null;
    const first = at(firstNames, i);
    const last = at(lastNames, i * 7 + Math.floor(i / firstNames.length) * 3);
    const created = daysAgo(260 - i * 4);
    const domain = company?.domain ?? pick(["example.com", "example.org", "example.net"]);
    contacts.push({
      id: idAt(created),
      first_name: first,
      last_name: last,
      email: `${first}.${last}`.toLowerCase().replace(/[^a-z.]/g, "") + `@${domain}`,
      phone: rand() < 0.8 ? `+91 ${int(70, 99)}${int(100, 999)} ${String(int(0, 99999)).padStart(5, "0")}` : null,
      title: rand() < 0.9 ? pick(titles) : null,
      status: pick(statuses),
      company_id: company?.id ?? null,
      owner_id: company && rand() < 0.75 ? company.owner_id : pick(sellers).id,
      created_at: iso(created),
      updated_at: iso(created + int(0, 20) * DAY),
      deleted_at: null,
    });
  }

  // ---- Deals -----------------------------------------------------------------
  const dealTitles = [
    "Fleet telematics rollout",
    "Annual platform license",
    "Warehouse analytics pilot",
    "Enterprise renewal",
    "Data residency add-on",
    "Regional expansion",
    "Support plan upgrade",
    "Onboarding services",
    "Multi-site deployment",
    "Security review package",
    "API access tier",
    "Seasonal capacity",
    "Procurement integration",
    "Executive dashboard suite",
    "Field team seats",
    "Compliance module",
    "Partner portal build",
    "Migration from legacy CRM",
    "Forecasting add-on",
    "Training workshop series",
    "Three-year enterprise agreement",
    "Pilot extension",
    "Customer success retainer",
    "Inventory sync connector",
    "Board reporting pack",
    "North India rollout",
    "Mobile field app",
    "Usage-based overage",
    "Sandbox environments",
    "Premium SLA",
    "Integration hub",
    "Analytics seats expansion",
  ];
  const stagePlan: DealStage[] = [
    "lead",
    "lead",
    "lead",
    "lead",
    "lead",
    "lead",
    "lead",
    "qualified",
    "qualified",
    "qualified",
    "qualified",
    "qualified",
    "qualified",
    "proposal",
    "proposal",
    "proposal",
    "proposal",
    "proposal",
    "proposal",
    "negotiation",
    "negotiation",
    "negotiation",
    "negotiation",
    "negotiation",
    "won",
    "won",
    "won",
    "won",
    "won",
    "lost",
    "lost",
    "lost",
  ];
  const lostReasons = [
    "Went with a competitor on price",
    "No budget this fiscal year",
    "Project paused after reorg; revisit next quarter",
  ];
  const positions = new Map<DealStage, number>();
  const deals: DealRow[] = stagePlan.map((stage, i) => {
    const company = at(companies, i * 5);
    const companyContacts = contacts.filter((c) => c.company_id === company.id);
    const created = daysAgo(120 - i * 3);
    const owner =
      i < 12
        ? at([rep1, rep2, manager, rep1], i)
        : rand() < 0.75
          ? users.find((u) => u.id === company.owner_id)!
          : pick(sellers);
    const position = positions.get(stage) ?? 0;
    positions.set(stage, position + 1);
    const amount = int(6, 240) * 50_000_00 + (rand() < 0.2 ? 25_000_00 : 0);
    let closedAt: string | null = null;
    if (stage === "won")
      closedAt = iso(
        i % 2 === 0
          ? startOfToday - int(0, Math.max(0, now.getUTCDate() - 2)) * DAY + 15 * 3600_000
          : daysAgo(int(35, 80)),
      );
    if (stage === "lost") closedAt = iso(daysAgo(int(5, 60)));
    if (closedAt && Date.parse(closedAt) > nowMs) closedAt = iso(nowMs - 3600_000);
    return {
      id: idAt(created),
      title: at(dealTitles, i),
      amount_cents: amount,
      currency: i === 9 ? "USD" : i === 21 ? "AED" : "INR",
      stage,
      position,
      expected_close_date: stage === "won" || stage === "lost" ? null : dateOnly(nowMs + int(-10, 75) * DAY),
      closed_at: closedAt,
      lost_reason: stage === "lost" ? at(lostReasons, i) : null,
      company_id: company.id,
      contact_id: companyContacts.length > 0 && rand() < 0.85 ? pick(companyContacts).id : null,
      owner_id: owner.id,
      created_at: iso(created),
      updated_at: iso(created + int(1, 20) * DAY),
      deleted_at: null,
    };
  });

  // ---- Activities ------------------------------------------------------------
  const notes = [
    ["Discovery call recap", "Three sites, ~140 drivers. Main pain is manual route reconciliation every Friday."],
    ["Pricing feedback", "Procurement wants a multi-year discount; finance signs off above ₹50 lakh."],
    ["Champion identified", "Ops lead is pushing internally; needs a one-pager for the CFO."],
    ["Security questionnaire received", "Due back by end of month. Looping in the solutions team."],
    ["Competitor in the mix", "They are also evaluating an incumbent tool. Our edge: integrations."],
    ["Renewal risk", "Usage dropped 20% last quarter after their reorg."],
  ];
  const calls = ["Intro call", "Follow-up call", "Pricing walkthrough", "Check-in with champion", "Reference call"];
  const emails = ["Sent proposal deck", "Shared case study", "Sent contract redlines", "Recap email after demo"];
  const meetings = ["On-site demo", "Quarterly business review", "Executive alignment meeting", "Technical deep dive"];
  const taskSubjects = [
    "Send revised proposal",
    "Book demo with IT",
    "Follow up on contract",
    "Prepare QBR deck",
    "Confirm budget owner",
    "Share security docs",
    "Draft mutual action plan",
    "Call back about pricing",
    "Send onboarding timeline",
    "Schedule renewal review",
    "Collect sign-off",
    "Update forecast notes",
  ];
  const activities: ActivityRow[] = [];
  const subjectFor = (i: number): Pick<ActivityRow, "subject_type" | "subject_id" | "owner_id"> => {
    const kind = i % 3;
    if (kind === 0) {
      const d = at(deals, i);
      return { subject_type: "deal", subject_id: d.id, owner_id: d.owner_id };
    }
    if (kind === 1) {
      const c = at(companies, i);
      return { subject_type: "company", subject_id: c.id, owner_id: c.owner_id };
    }
    const p = at(contacts, i);
    return { subject_type: "contact", subject_id: p.id, owner_id: p.owner_id };
  };

  // 40 logged interactions in the past.
  for (let i = 0; i < 40; i++) {
    const created = daysAgo(Math.floor(i * 1.6), 9 + (i % 8));
    const subject = subjectFor(i);
    const kind = at(["note", "call", "email", "meeting"] as const, i);
    const [title, body] =
      kind === "note"
        ? at(notes, i)
        : kind === "call"
          ? [at(calls, i), rand() < 0.6 ? "Spoke for 20 minutes. Next step agreed." : null]
          : kind === "email"
            ? [at(emails, i), null]
            : [at(meetings, i), rand() < 0.5 ? "Attendees: ops, finance, IT." : null];
    activities.push({
      id: idAt(created),
      kind,
      subject: title as string,
      body: body as string | null,
      due_at: kind === "meeting" ? iso(created) : null,
      completed_at: null,
      ...subject,
      assignee_id: null,
      created_at: iso(created),
    });
  }

  // 24 tasks: overdue, due today, upcoming, completed, spread across people.
  const assignees = [rep1, rep1, rep2, manager, rep1, rep2];
  const taskDue: [number, boolean][] = [
    [-3, false],
    [-1, false],
    [-6, false],
    [-2, false],
    [0, false],
    [0, false],
    [0, false],
    [0, false],
    [1, false],
    [2, false],
    [4, false],
    [7, false],
    [10, false],
    [3, false],
    [14, false],
    [-4, true],
    [-2, true],
    [-1, true],
    [-8, true],
    [0, true],
    [-5, true],
    [-3, true],
    [1, false],
    [5, false],
  ];
  taskDue.forEach(([offset, done], i) => {
    const subject = subjectFor(i + 2);
    const assignee = at(assignees, i);
    const due = startOfToday + offset * DAY + (offset === 0 ? (i % 2 === 0 ? 17 : 11) : 12) * 3600_000;
    const created = Math.min(due - (3 + (i % 4)) * DAY, nowMs - DAY - 3600_000 * i);
    activities.push({
      id: idAt(created),
      kind: "task",
      subject: at(taskSubjects, i),
      body: rand() < 0.4 ? "Check the latest notes before reaching out." : null,
      due_at: iso(due),
      completed_at: done ? iso(Math.min(due, nowMs - 1800_000)) : null,
      ...subject,
      // Most tasks are created by the person who does them; every fifth one was assigned by the
      // manager, which the assignee can still complete but not edit.
      owner_id: i % 5 === 0 && assignee.id !== manager.id ? manager.id : assignee.id,
      assignee_id: assignee.id,
      created_at: iso(created),
    });
  });

  // ---- Notifications ---------------------------------------------------------
  const notifications: NotificationRow[] = [];
  const pushNote = (user: UserRow, minutesAgo: number, type: string, data: Record<string, unknown>, read: boolean) => {
    const at = nowMs - minutesAgo * 60_000;
    notifications.push({
      id: idAt(at),
      user_id: user.id,
      type,
      data,
      read_at: read ? iso(at + 60_000) : null,
      created_at: iso(at),
    });
  };
  const wonDeal = deals.find((d) => d.stage === "won")!;
  const movedDeal = at(deals, 3);
  for (const user of users) {
    const own = deals.filter((d) => d.owner_id === user.id && d.stage !== "won" && d.stage !== "lost");
    if (own[0]) {
      pushNote(
        user,
        42,
        "deal.assigned",
        {
          title: "Deal assigned to you",
          message: `${manager.name} assigned you “${own[0].title}”`,
          deal_id: own[0].id,
          url: `/deals/${own[0].id}`,
        },
        false,
      );
    }
    pushNote(
      user,
      60 * 5,
      "deal.won",
      {
        title: "Deal won",
        message: `“${wonDeal.title}” was marked won`,
        deal_id: wonDeal.id,
        url: `/deals/${wonDeal.id}`,
      },
      user.id !== rep1.id,
    );
    pushNote(
      user,
      60 * 20,
      "tasks.due_today",
      {
        title: "Tasks due today",
        message: "You have tasks due today",
        url: "/tasks?tab=today",
      },
      false,
    );
    pushNote(
      user,
      60 * 50,
      "deal.updated",
      {
        title: "Deal moved",
        message: `${admin.name} moved “${movedDeal.title}” to ${movedDeal.stage}`,
        deal_id: movedDeal.id,
        url: `/deals/${movedDeal.id}`,
      },
      true,
    );
  }

  // ---- Audit log -------------------------------------------------------------
  const audit: AuditRow[] = [];
  const pushAudit = (
    actor: UserRow,
    minutesAgo: number,
    action: string,
    subjectType: string | null,
    subjectId: string | null,
    oldValues: Record<string, unknown> | null,
    newValues: Record<string, unknown> | null,
    kind = "model",
  ) => {
    const at = nowMs - minutesAgo * 60_000;
    const recorded = auditAction(kind, action, subjectType);
    audit.push({
      id: idAt(at),
      kind: recorded.kind,
      action: recorded.action,
      actor_id: actor.id,
      subject_type: recorded.subject ? subjectType : null,
      subject_id: recorded.subject ? subjectId : null,
      old_values: oldValues,
      new_values: newValues,
      request_id: idAt(at),
      created_at: iso(at),
    });
  };
  users.forEach((u, i) => pushAudit(u, 60 * (3 + i * 7), "login", "User", u.id, null, null, "auth"));
  deals.slice(0, 10).forEach((d, i) => {
    const actor = users.find((u) => u.id === d.owner_id)!;
    pushAudit(actor, 60 * (6 + i * 9), "updated", "Deal", d.id, { stage: "lead" }, { stage: d.stage });
  });
  companies.slice(0, 6).forEach((c, i) => {
    const actor = users.find((u) => u.id === c.owner_id)!;
    pushAudit(actor, 60 * 24 * (2 + i), "created", "Company", c.id, null, { name: c.name, industry: c.industry });
  });
  contacts.slice(0, 5).forEach((c, i) => {
    const actor = users.find((u) => u.id === c.owner_id)!;
    pushAudit(actor, 60 * 24 * (3 + i), "updated", "Contact", c.id, { status: "lead" }, { status: c.status });
  });
  pushAudit(admin, 60 * 24 * 9, "role_assigned", "user", rep2.id, null, { role: "rep" }, "access");
  audit.sort((a, b) => b.created_at.localeCompare(a.created_at));

  return { users, companies, contacts, deals, activities, notifications, audit };
}
