-- Keel benchmark database: schema and deterministic seed data (SPEC.md §2).
-- Runs once in the template database `bench_template`; every app gets a
-- fresh copy (`CREATE DATABASE bench TEMPLATE bench_template`).

CREATE TABLE world (
    id integer PRIMARY KEY,
    randomnumber integer NOT NULL
);

INSERT INTO world (id, randomnumber)
SELECT i, ((i * 7919) % 10000) + 1
FROM generate_series(1, 10000) AS i;

-- Keel's users table (keel-auth's migration), so Keel's own auth reads it.
CREATE TABLE users (
    id uuid PRIMARY KEY,
    name varchar NOT NULL,
    email varchar NOT NULL UNIQUE,
    password_hash varchar NOT NULL,
    email_verified_at timestamptz NULL,
    two_factor_secret text NULL,
    two_factor_recovery_codes text NULL,
    two_factor_confirmed_at timestamptz NULL,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);

INSERT INTO users (id, name, email, password_hash, email_verified_at, created_at, updated_at)
SELECT md5('user-' || i)::uuid,
       'User ' || i,
       'user' || i || '@bench.test',
       '$argon2id$v=19$m=19456,t=2,p=1$YmVuY2hzYWx0YmVuY2g$IxDcgYyB2ri1F5ehGxDXcyKQxvkcxWtaMnbJyIvrQ8c',
       timestamptz '2026-01-01 00:00:00+00',
       timestamptz '2026-01-01 00:00:00+00',
       timestamptz '2026-01-01 00:00:00+00'
FROM generate_series(1, 100) AS i;

CREATE TABLE deals (
    id uuid PRIMARY KEY,
    title varchar(200) NOT NULL,
    amount_cents bigint NOT NULL,
    stage varchar(20) NOT NULL,
    owner_id uuid NOT NULL REFERENCES users (id),
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);

INSERT INTO deals (id, title, amount_cents, stage, owner_id, created_at, updated_at)
SELECT md5('deal-' || i)::uuid,
       'Deal ' || i,
       (i::bigint * 104729) % 100000000,
       (ARRAY['lead', 'qualified', 'proposal', 'negotiation', 'won', 'lost'])[(i % 6) + 1],
       md5('user-' || ((i % 100) + 1))::uuid,
       timestamptz '2026-01-01 00:00:00+00' + i * interval '1 minute',
       timestamptz '2026-01-01 00:00:00+00' + i * interval '1 minute'
FROM generate_series(1, 1000) AS i;

CREATE INDEX deals_created_at_id_idx ON deals (created_at DESC, id DESC);
CREATE INDEX deals_owner_id_idx ON deals (owner_id);

VACUUM ANALYZE;
