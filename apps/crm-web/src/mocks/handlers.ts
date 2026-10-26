/**
 * MSW handlers implementing the Navis CRM API contract (spec §7, §9, §10) as the
 * browser sees it through the BFF: same-origin `/api/*`, the session held
 * "server side" (here: sessionStorage stands in for the httpOnly token
 * cookies), `POST /api/auth/login` returning only `{user}`, NestJS error bodies
 * (`{statusCode, message, error}`; validation = 400 with `message[]` and
 * `errors`), CASL authorization + 403 (the same rules the UI receives), login
 * throttling + 429, pagination, and `deal.updated` / `notification.created`
 * broadcasts on a window event bus.
 */
import { delay, http, HttpResponse, type HttpResponseResolver, type PathParams } from "msw";
import { createAppAbility, type AppAbility, type AppAction } from "@/lib/casl/ability";
import type {
  Activity,
  ActivityKind,
  ActivitySubjectType,
  ApiErrorBody,
  AuditEntry,
  Company,
  CompanyDetail,
  CompanySize,
  Contact,
  ContactStatus,
  Deal,
  DealStage,
  Page,
  Role,
  SubjectName,
  User,
} from "@/lib/api/types";
import {
  ACTIVITY_KINDS,
  COMPANY_SIZES,
  CONTACT_STATUSES,
  DEAL_STAGES,
  MAX_AMOUNT_CENTS,
  OPEN_STAGES,
  ROLES,
} from "@/lib/api/types";
import { rulesFor } from "./abilities";
import { db, failedAttempts, persist, recordFailedAttempt, sessionUserId, setSessionUser } from "./db";
import {
  auditAction,
  makeId,
  type ActivityRow,
  type CompanyRow,
  type ContactRow,
  type DealRow,
  type UserRow,
} from "./seed";

// ---- Plumbing ------------------------------------------------------------------

class HttpError {
  constructor(readonly response: Response) {}
}

const REASONS: Record<number, string> = {
  400: "Bad Request",
  401: "Unauthorized",
  403: "Forbidden",
  404: "Not Found",
  409: "Conflict",
  429: "Too Many Requests",
  500: "Internal Server Error",
};

function requestId(): string {
  return makeId(Date.now());
}

/** NestJS error body: `{statusCode, message, error}` (+ `errors` for validation). */
function errorResponse(
  status: number,
  message?: string | string[],
  errors?: Record<string, string[]>,
  headers?: Record<string, string>,
): Response {
  const body: ApiErrorBody = {
    statusCode: status,
    message: message ?? REASONS[status] ?? "Error",
    error: REASONS[status] ?? "Error",
    ...(errors ? { errors } : {}),
  };
  return HttpResponse.json(body, { status, headers: { "X-Request-Id": requestId(), ...headers } });
}

function fail(status: number, message?: string | string[], errors?: Record<string, string[]>): never {
  throw new HttpError(errorResponse(status, message, errors));
}

/** 400 with `message: string[]` and `errors: {field: [messages]}`, like Navis's validation pipe. */
function invalid(errors: Record<string, string[]>): void {
  if (Object.keys(errors).length > 0) fail(400, Object.values(errors).flat(), errors);
}

interface Ctx {
  request: Request;
  params: PathParams;
  url: URL;
  user: UserRow;
  ability: AppAbility;
  body: Record<string, unknown>;
}

type Handler = (ctx: Ctx) => Response | Promise<Response>;

function currentUser(): UserRow | null {
  const id = sessionUserId();
  return id ? (db.users.find((u) => u.id === id) ?? null) : null;
}

async function readBody(request: Request): Promise<Record<string, unknown>> {
  if (request.method === "GET" || request.method === "DELETE") return {};
  const text = await request.text();
  if (!text) return {};
  try {
    const parsed = JSON.parse(text);
    return typeof parsed === "object" && parsed !== null ? parsed : {};
  } catch {
    fail(400, "Malformed JSON body");
  }
}

const latency = () => delay(60 + Math.floor(Math.random() * 140));

/** Wraps a handler with latency, authentication and error handling. */
function route(handler: Handler, { auth = true } = {}): HttpResponseResolver {
  return async ({ request, params }) => {
    await latency();
    try {
      const user = currentUser();
      if (auth && !user) fail(401);
      const body = await readBody(request);
      const response = await handler({
        request,
        params,
        url: new URL(request.url),
        user: user as UserRow,
        ability: createAppAbility(user ? rulesFor(user) : []),
        body,
      });
      if (!response.headers.has("X-Request-Id")) response.headers.set("X-Request-Id", requestId());
      return response;
    } catch (error) {
      if (error instanceof HttpError) return error.response;
      console.error("[navis mock]", error);
      return errorResponse(500, "Internal server error");
    }
  };
}

/** Spec §9.3: only people who can `manage` a record type may set someone else as owner. */
function assertCanAssign(ctx: Ctx, subject: SubjectName, ownerId: string | undefined): void {
  if (ownerId && ownerId !== ctx.user.id && !ctx.ability.can("manage", subject)) {
    fail(403, "You can only assign records to yourself");
  }
}

function authorize(
  ctx: Ctx,
  action: AppAction,
  subject: SubjectName | (Record<string, unknown> & { __type: SubjectName }),
): void {
  // CASL types want API resources; mock rows carry the same fields the rules read.
  if (!ctx.ability.can(action, subject as never)) fail(403, "Forbidden resource");
}

function as<T extends SubjectName>(type: T, row: object): Record<string, unknown> & { __type: T } {
  return { ...(row as Record<string, unknown>), __type: type };
}

function paginate<T>(items: T[], url: URL): Page<T> {
  const perPage = Math.min(100, Math.max(1, Number(url.searchParams.get("per_page")) || 15));
  const total = items.length;
  const lastPage = Math.max(1, Math.ceil(total / perPage));
  const page = Math.max(1, Number(url.searchParams.get("page")) || 1);
  return {
    data: items.slice((page - 1) * perPage, page * perPage),
    meta: { current_page: page, per_page: perPage, total, last_page: lastPage },
  };
}

function nowIso(): string {
  return new Date().toISOString();
}

function newId(): string {
  return makeId(Date.now());
}

function contains(haystack: (string | null | undefined)[], needle: string): boolean {
  const n = needle.trim().toLowerCase();
  return haystack.some((h) => (h ?? "").toLowerCase().includes(n));
}

function str(value: unknown): string | null {
  if (value === undefined || value === null) return null;
  const s = String(value).trim();
  return s === "" ? null : s;
}

const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

function audit(
  ctx: Pick<Ctx, "user">,
  action: string,
  subjectType: string,
  subjectId: string,
  oldValues: object | null,
  newValues: object | null,
  kind = "model",
) {
  const recorded = auditAction(kind, action, subjectType);
  db.audit.unshift({
    id: newId(),
    kind: recorded.kind,
    action: recorded.action,
    actor_id: ctx.user?.id ?? null,
    subject_type: recorded.subject ? subjectType : null,
    subject_id: recorded.subject ? subjectId : null,
    old_values: oldValues as Record<string, unknown> | null,
    new_values: newValues as Record<string, unknown> | null,
    request_id: requestId(),
    created_at: nowIso(),
  });
}

function diff<T extends object>(before: T, after: T): [Partial<T>, Partial<T>] | null {
  const oldValues: Partial<T> = {};
  const newValues: Partial<T> = {};
  for (const key of Object.keys(after) as (keyof T)[]) {
    if (key === "updated_at") continue;
    if (before[key] !== after[key]) {
      oldValues[key] = before[key];
      newValues[key] = after[key];
    }
  }
  return Object.keys(newValues).length > 0 ? [oldValues, newValues] : null;
}

// ---- Broadcasting (mock real-time: a window event bus) ---------------------------

export const MOCK_BROADCAST_EVENT = "navis-mock:broadcast";

export interface MockBroadcast {
  channel: string;
  event: string;
  data: unknown;
}

function broadcast(channel: string, event: string, data: unknown): void {
  if (typeof window === "undefined") return;
  // Deliver after the HTTP response, as a real socket would.
  setTimeout(() => {
    window.dispatchEvent(new CustomEvent<MockBroadcast>(MOCK_BROADCAST_EVENT, { detail: { channel, event, data } }));
  }, 250);
}

function notify(userId: string, type: string, data: Record<string, unknown>): void {
  const row = { id: newId(), user_id: userId, type, data, read_at: null, created_at: nowIso() };
  db.notifications.unshift(row);
  // spec §9.7: the broadcast payload is the notification's data object.
  broadcast(`private-users.${userId}`, "notification.created", data);
}

// ---- Serialisers -------------------------------------------------------------------

function ownerRef(id: string | null): { id: string; name: string } | null {
  if (!id) return null;
  const u = db.users.find((x) => x.id === id);
  return { id, name: u?.name ?? "Former user" };
}

function toUser(u: UserRow): User {
  return {
    __type: "User",
    id: u.id,
    name: u.name,
    email: u.email,
    email_verified_at: u.email_verified_at,
    roles: [u.role],
    created_at: u.created_at,
  };
}

function toCompany(c: CompanyRow): Company {
  const { deleted_at: _deleted, ...rest } = c;
  void _deleted;
  return { __type: "Company", ...rest, owner: ownerRef(c.owner_id)! };
}

function toCompanyDetail(c: CompanyRow): CompanyDetail {
  const open = db.deals.filter((d) => !d.deleted_at && d.company_id === c.id && OPEN_STAGES.includes(d.stage));
  return {
    ...toCompany(c),
    contacts_count: db.contacts.filter((p) => !p.deleted_at && p.company_id === c.id).length,
    open_deals_count: open.length,
    open_deals_amount_cents: open.reduce((sum, d) => sum + d.amount_cents, 0),
  };
}

function fullName(p: ContactRow): string {
  return `${p.first_name} ${p.last_name}`;
}

function toContact(p: ContactRow): Contact {
  const { deleted_at: _deleted, ...rest } = p;
  void _deleted;
  const company = p.company_id ? db.companies.find((c) => c.id === p.company_id) : null;
  return {
    __type: "Contact",
    ...rest,
    full_name: fullName(p),
    company: company ? { id: company.id, name: company.name } : null,
    owner: ownerRef(p.owner_id)!,
  };
}

function toDeal(d: DealRow): Deal {
  const { deleted_at: _deleted, ...rest } = d;
  void _deleted;
  const company = db.companies.find((c) => c.id === d.company_id);
  const contact = d.contact_id ? db.contacts.find((p) => p.id === d.contact_id) : null;
  return {
    __type: "Deal",
    ...rest,
    probability: DEAL_STAGES.find((s) => s.stage === d.stage)?.probability ?? 0,
    company: { id: d.company_id, name: company?.name ?? "Deleted company" },
    contact: contact ? { id: contact.id, full_name: fullName(contact) } : null,
    owner: ownerRef(d.owner_id)!,
  };
}

function subjectLabel(type: ActivitySubjectType, id: string): string | null {
  if (type === "company") return db.companies.find((c) => c.id === id && !c.deleted_at)?.name ?? null;
  if (type === "contact") {
    const p = db.contacts.find((c) => c.id === id && !c.deleted_at);
    return p ? fullName(p) : null;
  }
  return db.deals.find((d) => d.id === id && !d.deleted_at)?.title ?? null;
}

function toActivity(a: ActivityRow): Activity {
  return {
    __type: "Activity",
    ...a,
    subject_label: subjectLabel(a.subject_type, a.subject_id) ?? "Deleted record",
    owner: ownerRef(a.owner_id)!,
    assignee: ownerRef(a.assignee_id),
  };
}

// ---- Lookups -------------------------------------------------------------------------

function findCompany(id: unknown): CompanyRow {
  const row = db.companies.find((c) => c.id === id && !c.deleted_at);
  if (!row) fail(404, "Company not found");
  return row;
}

function findContact(id: unknown): ContactRow {
  const row = db.contacts.find((c) => c.id === id && !c.deleted_at);
  if (!row) fail(404, "Contact not found");
  return row;
}

function findDeal(id: unknown): DealRow {
  const row = db.deals.find((d) => d.id === id && !d.deleted_at);
  if (!row) fail(404, "Deal not found");
  return row;
}

function findActivity(id: unknown): ActivityRow {
  const row = db.activities.find((a) => a.id === id);
  if (!row) fail(404, "Activity not found");
  return row;
}

function userExists(id: unknown): boolean {
  return typeof id === "string" && db.users.some((u) => u.id === id);
}

// ---- Validation --------------------------------------------------------------------

type Errors = Record<string, string[]>;

function add(errors: Errors, field: string, message: string) {
  (errors[field] ??= []).push(message);
}

function validateCompany(body: Record<string, unknown>, partial: boolean): Partial<CompanyRow> {
  const errors: Errors = {};
  const out: Partial<CompanyRow> = {};
  if (!partial || "name" in body) {
    const name = str(body.name);
    if (!name) add(errors, "name", "The name field is required.");
    else if (name.length > 255) add(errors, "name", "The name may not be greater than 255 characters.");
    else out.name = name;
  }
  for (const field of ["domain", "industry", "phone", "city", "country"] as const) {
    if (field in body) out[field] = str(body[field]);
  }
  if ("domain" in body && out.domain && !/^[a-z0-9.-]+\.[a-z]{2,}$/i.test(out.domain)) {
    add(errors, "domain", "The domain must be a valid domain name, like acme.com.");
  }
  if ("website" in body) {
    const website = str(body.website);
    if (website && !/^https?:\/\/\S+\.\S+/.test(website)) add(errors, "website", "The website must be a valid URL.");
    out.website = website;
  }
  if ("size" in body) {
    const size = str(body.size);
    if (size && !COMPANY_SIZES.includes(size as CompanySize)) add(errors, "size", "The selected size is invalid.");
    out.size = size as CompanySize | null;
  }
  if ("owner_id" in body && body.owner_id !== null && body.owner_id !== undefined) {
    if (!userExists(body.owner_id)) add(errors, "owner_id", "The selected owner is invalid.");
    else out.owner_id = body.owner_id as string;
  }
  invalid(errors);
  return out;
}

function validateContact(body: Record<string, unknown>, partial: boolean): Partial<ContactRow> {
  const errors: Errors = {};
  const out: Partial<ContactRow> = {};
  for (const field of ["first_name", "last_name"] as const) {
    if (!partial || field in body) {
      const value = str(body[field]);
      if (!value) add(errors, field, `The ${field.replace("_", " ")} field is required.`);
      else out[field] = value;
    }
  }
  if ("email" in body) {
    const email = str(body.email);
    if (email && !EMAIL.test(email)) add(errors, "email", "The email must be a valid email address.");
    out.email = email;
  }
  for (const field of ["phone", "title"] as const) {
    if (field in body) out[field] = str(body[field]);
  }
  if ("status" in body) {
    const status = str(body.status) ?? "lead";
    if (!CONTACT_STATUSES.includes(status as ContactStatus)) add(errors, "status", "The selected status is invalid.");
    out.status = status as ContactStatus;
  }
  if ("company_id" in body) {
    const companyId = str(body.company_id);
    if (companyId && !db.companies.some((c) => c.id === companyId && !c.deleted_at)) {
      add(errors, "company_id", "The selected company is invalid.");
    }
    out.company_id = companyId;
  }
  if ("owner_id" in body && body.owner_id) {
    if (!userExists(body.owner_id)) add(errors, "owner_id", "The selected owner is invalid.");
    else out.owner_id = body.owner_id as string;
  }
  invalid(errors);
  return out;
}

function validateDeal(body: Record<string, unknown>, partial: boolean): Partial<DealRow> {
  const errors: Errors = {};
  const out: Partial<DealRow> = {};
  if (!partial || "title" in body) {
    const title = str(body.title);
    if (!title) add(errors, "title", "The title field is required.");
    else out.title = title;
  }
  if (!partial || "company_id" in body) {
    const companyId = str(body.company_id);
    if (!companyId) add(errors, "company_id", "The company field is required.");
    else if (!db.companies.some((c) => c.id === companyId && !c.deleted_at))
      add(errors, "company_id", "The selected company is invalid.");
    else out.company_id = companyId;
  }
  if ("contact_id" in body) {
    const contactId = str(body.contact_id);
    if (contactId && !db.contacts.some((c) => c.id === contactId && !c.deleted_at))
      add(errors, "contact_id", "The selected contact is invalid.");
    out.contact_id = contactId;
  }
  if (!partial || "amount_cents" in body) {
    const amount = body.amount_cents;
    if (amount === undefined || amount === null || amount === "")
      add(errors, "amount_cents", "The amount field is required.");
    else if (typeof amount !== "number" || !Number.isInteger(amount))
      add(errors, "amount_cents", "The amount must be a whole number of cents.");
    else if (amount < 0) add(errors, "amount_cents", "The amount must be at least 0.");
    else if (amount > MAX_AMOUNT_CENTS)
      add(errors, "amount_cents", `amount_cents must not be greater than ${MAX_AMOUNT_CENTS}`);
    else out.amount_cents = amount;
  }
  if ("currency" in body || !partial) {
    const currency = (str(body.currency) ?? "USD").toUpperCase();
    if (!/^[A-Z]{3}$/.test(currency)) add(errors, "currency", "The currency must be a 3-letter ISO code.");
    else out.currency = currency;
  }
  if ("stage" in body) {
    const stage = str(body.stage) ?? "lead";
    if (!DEAL_STAGES.some((s) => s.stage === stage)) add(errors, "stage", "The selected stage is invalid.");
    else out.stage = stage as DealStage;
  }
  if ("expected_close_date" in body) {
    const date = str(body.expected_close_date);
    if (date && !/^\d{4}-\d{2}-\d{2}$/.test(date))
      add(errors, "expected_close_date", "The expected close date must be a date (YYYY-MM-DD).");
    out.expected_close_date = date;
  }
  if ("owner_id" in body && body.owner_id) {
    if (!userExists(body.owner_id)) add(errors, "owner_id", "The selected owner is invalid.");
    else out.owner_id = body.owner_id as string;
  }
  invalid(errors);
  return out;
}

function validateActivity(body: Record<string, unknown>, partial: boolean): Partial<ActivityRow> {
  const errors: Errors = {};
  const out: Partial<ActivityRow> = {};
  if (!partial || "kind" in body) {
    const kind = str(body.kind);
    if (!kind) add(errors, "kind", "The kind field is required.");
    else if (!ACTIVITY_KINDS.includes(kind as ActivityKind)) add(errors, "kind", "The selected kind is invalid.");
    else out.kind = kind as ActivityKind;
  }
  if (!partial || "subject" in body) {
    const subject = str(body.subject);
    if (!subject) add(errors, "subject", "The subject field is required.");
    else if (subject.length > 255) add(errors, "subject", "The subject may not be greater than 255 characters.");
    else out.subject = subject;
  }
  if ("body" in body) out.body = str(body.body);
  if ("due_at" in body) {
    const due = str(body.due_at);
    if (due && Number.isNaN(Date.parse(due))) add(errors, "due_at", "The due date is not a valid date.");
    out.due_at = due ? new Date(due).toISOString() : null;
  }
  if (!partial || "subject_type" in body || "subject_id" in body) {
    const type = str(body.subject_type);
    const id = str(body.subject_id);
    if (!type) add(errors, "subject_type", "The subject type field is required.");
    else if (!["company", "contact", "deal"].includes(type))
      add(errors, "subject_type", "The selected subject type is invalid.");
    if (!id) add(errors, "subject_id", "The subject id field is required.");
    if (type && id && ["company", "contact", "deal"].includes(type)) {
      if (subjectLabel(type as ActivitySubjectType, id) === null)
        add(errors, "subject_id", "The selected record does not exist.");
      else {
        out.subject_type = type as ActivitySubjectType;
        out.subject_id = id;
      }
    }
  }
  if ("assignee_id" in body) {
    const assignee = str(body.assignee_id);
    if (assignee && !userExists(assignee)) add(errors, "assignee_id", "The selected assignee is invalid.");
    out.assignee_id = assignee;
  }
  invalid(errors);
  return out;
}

// ---- Deals: ordering and moves ---------------------------------------------------------

function stageDeals(stage: DealStage, exceptId?: string): DealRow[] {
  return db.deals
    .filter((d) => !d.deleted_at && d.stage === stage && d.id !== exceptId)
    .sort((a, b) => a.position - b.position);
}

function renumber(rows: DealRow[]) {
  rows.forEach((row, index) => {
    row.position = index;
  });
}

function applyStage(deal: DealRow, stage: DealStage, lostReason: string | null) {
  const wasClosed = deal.stage === "won" || deal.stage === "lost";
  deal.stage = stage;
  if (stage === "won" || stage === "lost") {
    if (!wasClosed || deal.closed_at === null) deal.closed_at = nowIso();
    deal.lost_reason = stage === "lost" ? lostReason : null;
    deal.expected_close_date = deal.expected_close_date ?? null;
  } else {
    deal.closed_at = null;
    deal.lost_reason = null;
  }
}

function afterDealChange(ctx: Ctx, deal: DealRow, previous: DealRow) {
  deal.updated_at = nowIso();
  if (deal.owner_id !== previous.owner_id && deal.owner_id !== ctx.user.id) {
    notify(deal.owner_id, "deal.assigned", {
      title: "Deal assigned to you",
      message: `${ctx.user.name} assigned you “${deal.title}”`,
      deal_id: deal.id,
      url: `/deals/${deal.id}`,
    });
  }
  if (deal.stage === "won" && previous.stage !== "won") {
    notify(deal.owner_id, "deal.won", {
      title: "Deal won",
      message: `“${deal.title}” was marked won`,
      deal_id: deal.id,
      url: `/deals/${deal.id}`,
    });
  }
  const changes = diff(previous, deal);
  if (changes) audit(ctx, "updated", "Deal", deal.id, changes[0], changes[1]);
  broadcast("private-crm.deals", "deal.updated", { deal: toDeal(deal) });
}

// ---- Dashboard -------------------------------------------------------------------------

function dashboard(user: UserRow) {
  const deals = db.deals.filter((d) => !d.deleted_at);
  const now = new Date();
  // UTC day and month boundaries, like the API (spec §9.5).
  const monthStart = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), 1);
  const open = deals.filter((d) => OPEN_STAGES.includes(d.stage));
  const won = deals.filter((d) => d.stage === "won" && d.closed_at && Date.parse(d.closed_at) >= monthStart);
  const startToday = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate());
  // "My" tasks: assigned to me, or owned by me when unassigned (spec §9.6).
  const tasks = db.activities.filter(
    (a) =>
      a.kind === "task" &&
      !a.completed_at &&
      a.due_at &&
      (a.assignee_id === user.id || (a.assignee_id === null && a.owner_id === user.id)),
  );
  const at = (a: ActivityRow) => Date.parse(a.due_at!);
  return {
    pipeline: DEAL_STAGES.map(({ stage }) => {
      const rows = deals.filter((d) => d.stage === stage);
      return { stage, count: rows.length, amount_cents: rows.reduce((s, d) => s + d.amount_cents, 0) };
    }),
    won_this_month: { count: won.length, amount_cents: won.reduce((s, d) => s + d.amount_cents, 0) },
    open_deals: {
      count: open.length,
      amount_cents: open.reduce((s, d) => s + d.amount_cents, 0),
      weighted_amount_cents: Math.round(
        open.reduce(
          (s, d) => s + (d.amount_cents * (DEAL_STAGES.find((x) => x.stage === d.stage)?.probability ?? 0)) / 100,
          0,
        ),
      ),
    },
    tasks: {
      overdue: tasks.filter((a) => at(a) < startToday).length,
      due_today: tasks.filter((a) => at(a) >= startToday && at(a) < startToday + 86_400_000).length,
      upcoming: tasks.filter((a) => at(a) >= startToday + 86_400_000).length,
    },
    recent_activities: [...db.activities]
      .sort((a, b) => b.created_at.localeCompare(a.created_at))
      .slice(0, 10)
      .map(toActivity),
  };
}

// ---- CSV ---------------------------------------------------------------------------------

function csvCell(value: unknown): string {
  const s = value === null || value === undefined ? "" : String(value);
  return /[",\n\r]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
}

function filterContacts(url: URL): ContactRow[] {
  const search = url.searchParams.get("search");
  const companyId = url.searchParams.get("company_id");
  const status = url.searchParams.get("status");
  const ownerId = url.searchParams.get("owner_id");
  return db.contacts
    .filter((p) => !p.deleted_at)
    .filter((p) => !search || contains([fullName(p), p.email, p.title], search))
    .filter((p) => !companyId || p.company_id === companyId)
    .filter((p) => !status || p.status === status)
    .filter((p) => !ownerId || p.owner_id === ownerId)
    .sort((a, b) => a.last_name.localeCompare(b.last_name) || a.first_name.localeCompare(b.first_name));
}

// ---- Handlers ----------------------------------------------------------------------------

export const handlers = [
  // ---- Auth, as the BFF exposes it to the browser (tokens never leave the server)
  http.post(
    "/api/auth/login",
    route(
      (ctx) => {
        const email = str(ctx.body.email)?.toLowerCase() ?? "";
        const password = typeof ctx.body.password === "string" ? ctx.body.password : "";
        const errors: Errors = {};
        if (!email) add(errors, "email", "email should not be empty");
        else if (!EMAIL.test(email)) add(errors, "email", "email must be an email");
        if (!password) add(errors, "password", "password should not be empty");
        invalid(errors);
        if (failedAttempts(email) >= 5) {
          throw new HttpError(
            errorResponse(429, "Too many login attempts. Try again in 60 seconds.", undefined, { "Retry-After": "60" }),
          );
        }
        const user = db.users.find((u) => u.email.toLowerCase() === email);
        if (!user || user.password !== password) {
          recordFailedAttempt(email);
          fail(401, "Invalid credentials");
        }
        recordFailedAttempt(email, true);
        setSessionUser(user.id);
        audit({ user }, "login", "User", user.id, null, null, "auth");
        persist();
        // Navis returns {user, access_token, refresh_token, ...}; the BFF keeps the tokens.
        return HttpResponse.json({ user: toUser(user) });
      },
      { auth: false },
    ),
  ),

  http.post(
    "/api/auth/refresh",
    route(() => new HttpResponse(null, { status: 204 })),
  ),

  http.post(
    "/api/auth/logout",
    route(
      (ctx) => {
        if (ctx.user) audit(ctx, "logout", "User", ctx.user.id, null, null, "auth");
        setSessionUser(null);
        persist();
        return new HttpResponse(null, { status: 204 });
      },
      { auth: false },
    ),
  ),

  http.get(
    "/api/auth/me",
    route((ctx) => HttpResponse.json(toUser(ctx.user))),
  ),

  http.get(
    "/api/me/abilities",
    route((ctx) => {
      const rules = rulesFor(ctx.user);
      return HttpResponse.json(rules, { headers: { ETag: `"abilities-${ctx.user.id}-${ctx.user.role}"` } });
    }),
  ),

  http.get(
    "/api/v1/me",
    route((ctx) =>
      HttpResponse.json({ id: ctx.user.id, name: ctx.user.name, email: ctx.user.email, roles: [ctx.user.role] }),
    ),
  ),

  // ---- Users
  http.get(
    "/api/v1/users",
    route((ctx) => {
      const search = ctx.url.searchParams.get("search");
      const rows = db.users
        .filter((u) => !search || contains([u.name, u.email], search))
        .sort((a, b) => a.name.localeCompare(b.name))
        .map(toUser);
      return HttpResponse.json(paginate(rows, ctx.url));
    }),
  ),

  http.post(
    "/api/v1/users",
    route((ctx) => {
      authorize(ctx, "create", "User");
      const errors: Errors = {};
      const name = str(ctx.body.name);
      const email = str(ctx.body.email)?.toLowerCase() ?? null;
      const password = typeof ctx.body.password === "string" ? ctx.body.password : "";
      const role = str(ctx.body.role);
      if (!name) add(errors, "name", "The name field is required.");
      if (!email) add(errors, "email", "The email field is required.");
      else if (!EMAIL.test(email)) add(errors, "email", "The email must be a valid email address.");
      else if (db.users.some((u) => u.email.toLowerCase() === email))
        add(errors, "email", "The email has already been taken.");
      if (password.length < 8) add(errors, "password", "The password must be at least 8 characters.");
      if (!role || !ROLES.includes(role as Role)) add(errors, "role", "The selected role is invalid.");
      invalid(errors);
      const row: UserRow = {
        id: newId(),
        name: name!,
        email: email!,
        password,
        role: role as Role,
        email_verified_at: null,
        created_at: nowIso(),
      };
      db.users.push(row);
      audit(ctx, "created", "User", row.id, null, { name: row.name, email: row.email });
      audit(ctx, "role_assigned", "user", row.id, null, { role: row.role }, "access");
      persist();
      return HttpResponse.json(toUser(row), { status: 201 });
    }),
  ),

  http.patch(
    "/api/v1/users/:id",
    route((ctx) => {
      authorize(ctx, "update", "User");
      const row = db.users.find((u) => u.id === ctx.params.id);
      if (!row) fail(404, "User not found");
      const errors: Errors = {};
      const before = { name: row.name, roles: [row.role] };
      if ("name" in ctx.body) {
        const name = str(ctx.body.name);
        if (!name) add(errors, "name", "The name field is required.");
        else row.name = name;
      }
      if ("role" in ctx.body) {
        const role = str(ctx.body.role);
        if (!role || !ROLES.includes(role as Role)) add(errors, "role", "The selected role is invalid.");
        else row.role = role as Role;
      }
      invalid(errors);
      // Like crm-api: a new name is a `user.updated` row, a new role navis-access's `access.user_roles_synced`.
      if (before.name !== row.name) audit(ctx, "updated", "User", row.id, { name: before.name }, { name: row.name });
      if (before.roles[0] !== row.role) {
        audit(ctx, "user_roles_synced", "user", row.id, { roles: before.roles }, { roles: [row.role] }, "access");
      }
      persist();
      return HttpResponse.json(toUser(row));
    }),
  ),

  // ---- Companies
  http.get(
    "/api/v1/companies",
    route((ctx) => {
      const q = ctx.url.searchParams;
      const search = q.get("search");
      const ownerId = q.get("owner_id");
      const industry = q.get("industry");
      const sort = q.get("sort") ?? "name";
      const desc = sort.startsWith("-");
      const key = (desc ? sort.slice(1) : sort) as "name" | "created_at";
      if (!["name", "created_at"].includes(key))
        invalid({ sort: ["sort must be one of the following values: name, -name, created_at, -created_at"] });
      const rows = db.companies
        .filter((c) => !c.deleted_at)
        .filter((c) => !search || contains([c.name, c.domain], search))
        .filter((c) => !ownerId || c.owner_id === ownerId)
        .filter((c) => !industry || c.industry === industry)
        .sort((a, b) => (desc ? -1 : 1) * a[key].localeCompare(b[key]))
        .map(toCompany);
      return HttpResponse.json(paginate(rows, ctx.url));
    }),
  ),

  http.post(
    "/api/v1/companies",
    route((ctx) => {
      authorize(ctx, "create", "Company");
      const input = validateCompany(ctx.body, false);
      assertCanAssign(ctx, "Company", input.owner_id);
      const now = nowIso();
      const row: CompanyRow = {
        id: newId(),
        name: input.name!,
        domain: input.domain ?? null,
        industry: input.industry ?? null,
        size: input.size ?? null,
        phone: input.phone ?? null,
        website: input.website ?? null,
        city: input.city ?? null,
        country: input.country ?? null,
        owner_id: input.owner_id ?? ctx.user.id,
        created_at: now,
        updated_at: now,
        deleted_at: null,
      };
      db.companies.push(row);
      audit(ctx, "created", "Company", row.id, null, { name: row.name });
      persist();
      return HttpResponse.json(toCompany(row), { status: 201 });
    }),
  ),

  http.get(
    "/api/v1/companies/:id",
    route((ctx) => HttpResponse.json(toCompanyDetail(findCompany(ctx.params.id)))),
  ),

  http.patch(
    "/api/v1/companies/:id",
    route((ctx) => {
      const row = findCompany(ctx.params.id);
      authorize(ctx, "update", as("Company", row));
      const before = { ...row };
      const input = validateCompany(ctx.body, true);
      assertCanAssign(ctx, "Company", input.owner_id);
      Object.assign(row, input, { updated_at: nowIso() });
      const changes = diff(before, row);
      if (changes) audit(ctx, "updated", "Company", row.id, changes[0], changes[1]);
      persist();
      return HttpResponse.json(toCompany(row));
    }),
  ),

  http.delete(
    "/api/v1/companies/:id",
    route((ctx) => {
      const row = findCompany(ctx.params.id);
      authorize(ctx, "delete", as("Company", row));
      row.deleted_at = nowIso();
      // A soft delete: crm-api records it as an update of `deleted_at`.
      audit(ctx, "updated", "Company", row.id, { deleted_at: null }, { deleted_at: row.deleted_at });
      persist();
      return new HttpResponse(null, { status: 204 });
    }),
  ),

  // ---- Contacts
  http.get(
    "/api/v1/contacts",
    route((ctx) => HttpResponse.json(paginate(filterContacts(ctx.url).map(toContact), ctx.url))),
  ),

  http.get(
    "/api/v1/contacts/export",
    route((ctx) => {
      authorize(ctx, "export", "Contact");
      const header = [
        "id",
        "first_name",
        "last_name",
        "email",
        "phone",
        "title",
        "status",
        "company",
        "owner",
        "created_at",
      ];
      const lines = filterContacts(ctx.url).map((p) => {
        const c = toContact(p);
        return [
          c.id,
          c.first_name,
          c.last_name,
          c.email,
          c.phone,
          c.title,
          c.status,
          c.company?.name,
          c.owner.name,
          c.created_at,
        ]
          .map(csvCell)
          .join(",");
      });
      return new HttpResponse([header.join(","), ...lines].join("\r\n") + "\r\n", {
        headers: {
          "Content-Type": "text/csv; charset=utf-8",
          "Content-Disposition": 'attachment; filename="contacts.csv"',
        },
      });
    }),
  ),

  http.post(
    "/api/v1/contacts",
    route((ctx) => {
      authorize(ctx, "create", "Contact");
      const input = validateContact({ status: "lead", ...ctx.body }, false);
      assertCanAssign(ctx, "Contact", input.owner_id);
      const now = nowIso();
      const row: ContactRow = {
        id: newId(),
        first_name: input.first_name!,
        last_name: input.last_name!,
        email: input.email ?? null,
        phone: input.phone ?? null,
        title: input.title ?? null,
        status: input.status ?? "lead",
        company_id: input.company_id ?? null,
        owner_id: input.owner_id ?? ctx.user.id,
        created_at: now,
        updated_at: now,
        deleted_at: null,
      };
      db.contacts.push(row);
      audit(ctx, "created", "Contact", row.id, null, { first_name: row.first_name, last_name: row.last_name });
      persist();
      return HttpResponse.json(toContact(row), { status: 201 });
    }),
  ),

  http.get(
    "/api/v1/contacts/:id",
    route((ctx) => HttpResponse.json(toContact(findContact(ctx.params.id)))),
  ),

  http.patch(
    "/api/v1/contacts/:id",
    route((ctx) => {
      const row = findContact(ctx.params.id);
      authorize(ctx, "update", as("Contact", row));
      const before = { ...row };
      const input = validateContact(ctx.body, true);
      assertCanAssign(ctx, "Contact", input.owner_id);
      Object.assign(row, input, { updated_at: nowIso() });
      const changes = diff(before, row);
      if (changes) audit(ctx, "updated", "Contact", row.id, changes[0], changes[1]);
      persist();
      return HttpResponse.json(toContact(row));
    }),
  ),

  http.delete(
    "/api/v1/contacts/:id",
    route((ctx) => {
      const row = findContact(ctx.params.id);
      authorize(ctx, "delete", as("Contact", row));
      row.deleted_at = nowIso();
      // A soft delete: crm-api records it as an update of `deleted_at`.
      audit(ctx, "updated", "Contact", row.id, { deleted_at: null }, { deleted_at: row.deleted_at });
      persist();
      return new HttpResponse(null, { status: 204 });
    }),
  ),

  // ---- Deals
  http.get(
    "/api/v1/deals",
    route((ctx) => {
      const q = ctx.url.searchParams;
      const search = q.get("search");
      const rows = db.deals
        .filter((d) => !d.deleted_at)
        .filter((d) => !q.get("stage") || d.stage === q.get("stage"))
        .filter((d) => !q.get("owner_id") || d.owner_id === q.get("owner_id"))
        .filter((d) => !q.get("company_id") || d.company_id === q.get("company_id"))
        .filter((d) => !search || contains([d.title], search))
        .sort((a, b) => b.created_at.localeCompare(a.created_at))
        .map(toDeal);
      return HttpResponse.json(paginate(rows, ctx.url));
    }),
  ),

  http.get(
    "/api/v1/deals/board",
    route((ctx) => {
      const ownerId = ctx.url.searchParams.get("owner_id");
      return HttpResponse.json({
        stages: DEAL_STAGES.map(({ stage, label, probability }) => {
          const deals = stageDeals(stage).filter((d) => !ownerId || d.owner_id === ownerId);
          return {
            stage,
            label,
            probability,
            count: deals.length,
            amount_cents: deals.reduce((s, d) => s + d.amount_cents, 0),
            deals: deals.map(toDeal),
          };
        }),
      });
    }),
  ),

  http.post(
    "/api/v1/deals",
    route((ctx) => {
      authorize(ctx, "create", "Deal");
      const input = validateDeal(ctx.body, false);
      assertCanAssign(ctx, "Deal", input.owner_id);
      const now = nowIso();
      const stage = input.stage ?? "lead";
      const row: DealRow = {
        id: newId(),
        title: input.title!,
        amount_cents: input.amount_cents!,
        currency: input.currency ?? "USD",
        stage,
        position: stageDeals(stage).length,
        expected_close_date: input.expected_close_date ?? null,
        closed_at: stage === "won" || stage === "lost" ? now : null,
        lost_reason: null,
        company_id: input.company_id!,
        contact_id: input.contact_id ?? null,
        owner_id: input.owner_id ?? ctx.user.id,
        created_at: now,
        updated_at: now,
        deleted_at: null,
      };
      db.deals.push(row);
      audit(ctx, "created", "Deal", row.id, null, {
        title: row.title,
        amount_cents: row.amount_cents,
        stage: row.stage,
      });
      if (row.owner_id !== ctx.user.id) {
        notify(row.owner_id, "deal.assigned", {
          title: "Deal assigned to you",
          message: `${ctx.user.name} assigned you “${row.title}”`,
          deal_id: row.id,
          url: `/deals/${row.id}`,
        });
      }
      broadcast("private-crm.deals", "deal.updated", { deal: toDeal(row) });
      persist();
      return HttpResponse.json(toDeal(row), { status: 201 });
    }),
  ),

  http.get(
    "/api/v1/deals/:id",
    route((ctx) => HttpResponse.json(toDeal(findDeal(ctx.params.id)))),
  ),

  http.patch(
    "/api/v1/deals/:id",
    route((ctx) => {
      const row = findDeal(ctx.params.id);
      authorize(ctx, "update", as("Deal", row));
      const before = { ...row };
      const input = validateDeal(ctx.body, true);
      assertCanAssign(ctx, "Deal", input.owner_id);
      const { stage, ...rest } = input;
      // Changing the stage through PATCH needs `move` too (reps can't reopen closed deals).
      if (stage && stage !== row.stage) authorize(ctx, "move", as("Deal", row));
      Object.assign(row, rest);
      if (stage && stage !== row.stage) {
        const source = stageDeals(row.stage, row.id);
        applyStage(row, stage, null);
        row.position = stageDeals(stage, row.id).length;
        renumber(source);
      }
      afterDealChange(ctx, row, before);
      persist();
      return HttpResponse.json(toDeal(row));
    }),
  ),

  http.delete(
    "/api/v1/deals/:id",
    route((ctx) => {
      const row = findDeal(ctx.params.id);
      authorize(ctx, "delete", as("Deal", row));
      row.deleted_at = nowIso();
      renumber(stageDeals(row.stage, row.id));
      audit(ctx, "deleted", "Deal", row.id, { title: row.title }, null);
      broadcast("private-crm.deals", "deal.updated", { deal: toDeal(row) });
      persist();
      return new HttpResponse(null, { status: 204 });
    }),
  ),

  http.post(
    "/api/v1/deals/:id/move",
    route((ctx) => {
      const row = findDeal(ctx.params.id);
      authorize(ctx, "move", as("Deal", row));
      const errors: Errors = {};
      const stage = str(ctx.body.stage) as DealStage | null;
      const lostReason = str(ctx.body.lost_reason);
      if (!stage) add(errors, "stage", "The stage field is required.");
      else if (!DEAL_STAGES.some((s) => s.stage === stage)) add(errors, "stage", "The selected stage is invalid.");
      if (stage === "lost" && !lostReason) add(errors, "lost_reason", "A reason is required when a deal is lost.");
      const position = ctx.body.position;
      if (
        position !== undefined &&
        position !== null &&
        (typeof position !== "number" || !Number.isInteger(position) || position < 0)
      ) {
        add(errors, "position", "The position must be a non-negative integer.");
      }
      invalid(errors);

      const before = { ...row };
      const source = stageDeals(row.stage, row.id);
      const target = stageDeals(stage!, row.id);
      const index = typeof position === "number" ? Math.min(position, target.length) : target.length;
      applyStage(row, stage!, lostReason);
      target.splice(index, 0, row);
      renumber(target);
      if (before.stage !== stage) renumber(source);
      afterDealChange(ctx, row, before);
      persist();
      return HttpResponse.json(toDeal(row));
    }),
  ),

  // ---- Activities
  http.get(
    "/api/v1/activities",
    route((ctx) => {
      const q = ctx.url.searchParams;
      const now = new Date();
      const startToday = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate());
      const due = q.get("due");
      const completed = q.get("completed");
      let rows = db.activities
        .filter((a) => subjectLabel(a.subject_type, a.subject_id) !== null)
        .filter((a) => !q.get("subject_type") || a.subject_type === q.get("subject_type"))
        .filter((a) => !q.get("subject_id") || a.subject_id === q.get("subject_id"))
        .filter((a) => !q.get("kind") || a.kind === q.get("kind"))
        .filter((a) => !q.get("assignee_id") || a.assignee_id === q.get("assignee_id"))
        .filter((a) => completed === null || (completed === "true") === (a.completed_at !== null))
        .filter((a) => {
          if (!due) return true;
          // spec §9.5: due filters apply to open tasks, in UTC days.
          if (!a.due_at || a.kind !== "task") return false;
          const t = Date.parse(a.due_at);
          if (due === "overdue") return t < startToday;
          if (due === "today") return t >= startToday && t < startToday + 86_400_000;
          if (due === "upcoming") return t >= startToday + 86_400_000;
          return true;
        });
      if (completed === "true") rows = rows.sort((a, b) => (b.completed_at ?? "").localeCompare(a.completed_at ?? ""));
      else if (due || q.get("kind") === "task")
        rows = rows.sort((a, b) => (a.due_at ?? "9").localeCompare(b.due_at ?? "9"));
      else rows = rows.sort((a, b) => b.created_at.localeCompare(a.created_at));
      return HttpResponse.json(paginate(rows.map(toActivity), ctx.url));
    }),
  ),

  http.post(
    "/api/v1/activities",
    route((ctx) => {
      authorize(ctx, "create", "Activity");
      const input = validateActivity(ctx.body, false);
      const row: ActivityRow = {
        id: newId(),
        kind: input.kind!,
        subject: input.subject!,
        body: input.body ?? null,
        due_at: input.due_at ?? null,
        completed_at: null,
        subject_type: input.subject_type!,
        subject_id: input.subject_id!,
        owner_id: ctx.user.id,
        assignee_id: input.assignee_id ?? null,
        created_at: nowIso(),
      };
      db.activities.push(row);
      audit(ctx, "created", "Activity", row.id, null, { kind: row.kind, subject: row.subject });
      persist();
      return HttpResponse.json(toActivity(row), { status: 201 });
    }),
  ),

  http.patch(
    "/api/v1/activities/:id",
    route((ctx) => {
      const row = findActivity(ctx.params.id);
      authorize(ctx, "update", as("Activity", row));
      const before = { ...row };
      Object.assign(row, validateActivity(ctx.body, true));
      const changes = diff(before, row);
      if (changes) audit(ctx, "updated", "Activity", row.id, changes[0], changes[1]);
      persist();
      return HttpResponse.json(toActivity(row));
    }),
  ),

  http.post(
    "/api/v1/activities/:id/complete",
    route((ctx) => {
      const row = findActivity(ctx.params.id);
      authorize(ctx, "complete", as("Activity", row));
      if (!row.completed_at) {
        row.completed_at = nowIso();
        audit(ctx, "completed", "Activity", row.id, { completed_at: null }, { completed_at: row.completed_at });
      }
      persist();
      return HttpResponse.json(toActivity(row));
    }),
  ),

  http.delete(
    "/api/v1/activities/:id",
    route((ctx) => {
      const row = findActivity(ctx.params.id);
      authorize(ctx, "delete", as("Activity", row));
      db.activities = db.activities.filter((a) => a.id !== row.id);
      audit(ctx, "deleted", "Activity", row.id, { subject: row.subject }, null);
      persist();
      return new HttpResponse(null, { status: 204 });
    }),
  ),

  // ---- Dashboard
  http.get(
    "/api/v1/dashboard",
    route((ctx) => HttpResponse.json(dashboard(ctx.user))),
  ),

  // ---- Notifications
  http.get(
    "/api/notifications",
    route((ctx) => {
      const unread = ctx.url.searchParams.get("unread") === "true";
      const rows = db.notifications
        .filter((n) => n.user_id === ctx.user.id && (!unread || n.read_at === null))
        .sort((a, b) => b.created_at.localeCompare(a.created_at))
        .map(({ user_id: _user, ...n }) => {
          void _user;
          return n;
        });
      return HttpResponse.json(paginate(rows, ctx.url));
    }),
  ),

  http.get(
    "/api/notifications/unread-count",
    route((ctx) =>
      HttpResponse.json({
        count: db.notifications.filter((n) => n.user_id === ctx.user.id && n.read_at === null).length,
      }),
    ),
  ),

  http.post(
    "/api/notifications/read-all",
    route((ctx) => {
      const now = nowIso();
      for (const n of db.notifications) if (n.user_id === ctx.user.id && !n.read_at) n.read_at = now;
      persist();
      return new HttpResponse(null, { status: 204 });
    }),
  ),

  http.post(
    "/api/notifications/:id/read",
    route((ctx) => {
      const row = db.notifications.find((n) => n.id === ctx.params.id && n.user_id === ctx.user.id);
      if (!row) fail(404, "Notification not found");
      row.read_at ??= nowIso();
      persist();
      return new HttpResponse(null, { status: 204 });
    }),
  ),

  // ---- Audit log
  http.get(
    "/api/audit",
    route((ctx) => {
      if (ctx.user.role !== "admin") fail(403, "Only admins can view the audit log");
      const q = ctx.url.searchParams;
      const rows: AuditEntry[] = db.audit
        .filter((e) => !q.get("actor_id") || e.actor_id === q.get("actor_id"))
        .filter((e) => !q.get("subject_type") || e.subject_type === q.get("subject_type"))
        .filter((e) => !q.get("subject_id") || e.subject_id === q.get("subject_id"))
        .filter((e) => !q.get("action") || e.action === q.get("action"))
        .sort((a, b) => b.created_at.localeCompare(a.created_at))
        .map((e) => ({ ...e, kind: e.kind as AuditEntry["kind"], ip: null, user_agent: null, metadata: {} }));
      return HttpResponse.json(paginate(rows, ctx.url));
    }),
  ),

  // ---- Broadcasting auth (the mock real-time bus doesn't need it, but keep the contract)
  http.post(
    "/broadcasting/auth",
    route((ctx) => {
      const channel = str(ctx.body.channel_name) ?? "";
      if (channel.startsWith("private-users.") && channel !== `private-users.${ctx.user.id}`)
        fail(403, "Forbidden resource");
      return HttpResponse.json({ auth: `navis-crm-key:mock-signature` });
    }),
  ),
];
