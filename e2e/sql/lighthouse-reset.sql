-- Starts Lighthouse from nothing: `migrate:fresh --seed` runs inside the
-- app's own process type, so Lighthouse records its queries (the DDL, the
-- seeder's inserts) like any other work. A tool "before any traffic" should
-- show none of that.
TRUNCATE lighthouse_occurrences, lighthouse_samples, lighthouse_aggregates, lighthouse_alerts, lighthouse_issues;
