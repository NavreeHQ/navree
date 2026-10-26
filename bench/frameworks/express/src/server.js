// Express 5 + node-postgres + jsonwebtoken + zod, one worker per CPU (SPEC.md).
import cluster from "node:cluster";
import { randomUUID } from "node:crypto";
import { availableParallelism } from "node:os";
import express from "express";
import jwt from "jsonwebtoken";
import pg from "pg";
import { z } from "zod";

const WORKERS = availableParallelism();
const MAX_DB_CONNECTIONS = 64;

if (cluster.isPrimary) {
  for (let i = 0; i < WORKERS; i++) cluster.fork();
  cluster.on("exit", () => cluster.fork());
} else {
  start();
}

function start() {
  const pool = new pg.Pool({
    connectionString: process.env.DATABASE_URL,
    max: Math.max(1, Math.floor(MAX_DB_CONNECTIONS / WORKERS)),
  });
  const secret = process.env.JWT_SECRET;
  const issuer = process.env.JWT_ISSUER ?? "keel";
  const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

  const app = express();
  app.disable("x-powered-by");

  const randomId = () => Math.floor(Math.random() * 10000) + 1;
  const worldRow = async () => {
    const { rows } = await pool.query("SELECT id, randomnumber FROM world WHERE id = $1", [randomId()]);
    return { id: rows[0].id, randomNumber: rows[0].randomnumber };
  };

  app.get("/json", (req, res) => {
    res.json({ message: "Hello, World!" });
  });

  app.get("/db", async (req, res) => {
    res.json(await worldRow());
  });

  app.get("/queries", async (req, res) => {
    const n = Math.min(500, Math.max(1, Number.parseInt(req.query.queries, 10) || 1));
    res.json(await Promise.all(Array.from({ length: n }, worldRow)));
  });

  // Bearer JWT, then the user row (401 for anything wrong).
  const authenticate = async (req, res, next) => {
    const header = req.get("authorization") ?? "";
    const token = header.startsWith("Bearer ") ? header.slice(7) : null;
    let claims;
    try {
      claims = token && jwt.verify(token, secret, { algorithms: ["HS256"], issuer });
    } catch {
      claims = null;
    }
    if (!claims || typeof claims.sub !== "string" || !UUID.test(claims.sub)) {
      return res.status(401).json({ message: "Unauthenticated." });
    }
    const { rows } = await pool.query("SELECT id, name FROM users WHERE id = $1", [claims.sub]);
    if (rows.length === 0) return res.status(401).json({ message: "Unauthenticated." });
    req.user = rows[0];
    next();
  };

  const STAGES = ["lead", "qualified", "proposal", "negotiation", "won", "lost"];
  const listQuery = z.object({
    page: z.coerce.number().int().min(1).default(1),
    per_page: z.coerce.number().int().min(1).max(100).default(20),
  });
  const createBody = z.object({
    title: z.string().min(1).max(200),
    amount_cents: z.number().int().min(0).max(1_000_000_000_000),
    stage: z.enum(STAGES),
  });

  const dealResource = (row) => ({
    id: row.id,
    title: row.title,
    amount_cents: Number(row.amount_cents),
    stage: row.stage,
    owner: { id: row.owner_id, name: row.owner_name },
    created_at: row.created_at,
  });

  app.get("/api/deals", authenticate, async (req, res) => {
    const parsed = listQuery.safeParse(req.query);
    if (!parsed.success) return res.status(400).json({ message: "Validation failed", errors: parsed.error.issues });
    const { page, per_page } = parsed.data;
    const [count, rows] = await Promise.all([
      pool.query("SELECT count(*)::int AS total FROM deals"),
      pool.query(
        `SELECT d.id, d.title, d.amount_cents, d.stage, d.created_at, u.id AS owner_id, u.name AS owner_name
           FROM deals d JOIN users u ON u.id = d.owner_id
          ORDER BY d.created_at DESC, d.id DESC
          LIMIT $1 OFFSET $2`,
        [per_page, (page - 1) * per_page],
      ),
    ]);
    const total = count.rows[0].total;
    res.json({
      data: rows.rows.map(dealResource),
      meta: { current_page: page, per_page, total, last_page: Math.max(1, Math.ceil(total / per_page)) },
    });
  });

  app.post("/api/deals", authenticate, express.json(), async (req, res) => {
    const parsed = createBody.safeParse(req.body);
    if (!parsed.success) return res.status(400).json({ message: "Validation failed", errors: parsed.error.issues });
    const { title, amount_cents, stage } = parsed.data;
    const now = new Date();
    const { rows } = await pool.query(
      `INSERT INTO deals (id, title, amount_cents, stage, owner_id, created_at, updated_at)
       VALUES ($1, $2, $3, $4, $5, $6, $6)
       RETURNING id, title, amount_cents, stage, created_at`,
      [randomUUID(), title, amount_cents, stage, req.user.id, now],
    );
    res.status(201).json(dealResource({ ...rows[0], owner_id: req.user.id, owner_name: req.user.name }));
  });

  // Malformed JSON bodies and anything thrown: JSON errors.
  app.use((err, req, res, next) => {
    const status = err.status ?? err.statusCode ?? 500;
    res.status(status).json({ message: status === 500 ? "Internal Server Error" : err.message });
  });

  app.listen(8080, "0.0.0.0");
}
