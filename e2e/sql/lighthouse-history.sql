-- Back-dated history for Lighthouse's 24-hour and 7-day charts.
--
-- NOT LIVE TRAFFIC. Lighthouse keeps per-hour aggregates; a run of a few
-- minutes fills one or two of them, so the longer charts would show a
-- single bar. This file copies the hour rows that the run recorded (requests,
-- queries, jobs, outgoing calls, exceptions, per-issue counts) into the 167
-- hours before the current one, scaled by the shape of a working day (quiet
-- nights, a midday peak, about 40 % of that on weekends) and by a
-- deterministic +-15 % noise per series and hour. Histograms and maxima are
-- copied as they are, so percentiles keep the recorded distribution. Issues
-- get an earlier `first_seen` and counts that include the copies.
--
-- Rows from CLI commands (`migrate:fresh`, `db:seed`) are not copied. Running
-- it twice changes nothing: the unique key of the aggregates ignores the
-- second set.

BEGIN;

CREATE TEMP TABLE history_series ON COMMIT DROP AS
SELECT kind, key_hash, key,
       (array_agg(process ORDER BY count DESC))[1] AS process,
       sum(count) AS count, sum(errors) AS errors, sum(extra) AS extra,
       sum(total_ms) AS total_ms, max(max_ms) AS max_ms,
       (array_agg(histogram ORDER BY count DESC))[1] AS histogram,
       (array_agg(detail ORDER BY count DESC) FILTER (WHERE detail IS NOT NULL))[1] AS detail
FROM lighthouse_aggregates
WHERE resolution = 3600
  AND process NOT LIKE '%/migrate:fresh/%'
  AND process NOT LIKE '%/db:seed/%'
GROUP BY kind, key_hash, key;

CREATE TEMP TABLE history_hours ON COMMIT DROP AS
SELECT n,
       date_trunc('hour', now()) - n * interval '1 hour' AS bucket,
       CASE WHEN extract(isodow FROM date_trunc('hour', now()) - n * interval '1 hour') >= 6 THEN 0.4 ELSE 1.0 END AS weekday,
       0.08 + 0.92 * power(greatest(0, sin(pi() * (((extract(hour FROM date_trunc('hour', now()) - n * interval '1 hour'))::int - 5 + 24) % 24) / 15.0)), 1.6) AS shape
FROM generate_series(1, 167) AS n;

INSERT INTO lighthouse_aggregates
  (id, resolution, bucket, kind, key_hash, key, process, count, errors, extra, total_ms, max_ms, histogram, detail)
SELECT gen_random_uuid(), 3600, h.bucket, s.kind, s.key_hash, s.key, s.process,
       round(s.count * f.factor)::bigint,
       round(s.errors * f.factor)::bigint,
       round(s.extra * f.factor)::bigint,
       s.total_ms * f.factor,
       s.max_ms, s.histogram, s.detail
FROM history_series s
CROSS JOIN history_hours h
CROSS JOIN LATERAL (
  SELECT (CASE WHEN s.kind IN ('request', 'query') THEN 3.0 ELSE 1.5 END)
         * h.shape * h.weekday
         * (0.85 + 0.30 * ((abs(hashtext(s.key_hash || h.n::text)) % 1000) / 1000.0)) AS factor
) f
WHERE round(s.count * f.factor) >= 1
ON CONFLICT (resolution, bucket, kind, key_hash, process) DO NOTHING;

-- Issues: older first sightings, and counts that include the history.
UPDATE lighthouse_issues i
SET first_seen = LEAST(i.first_seen, a.first_bucket),
    count = GREATEST(i.count, a.total)
FROM (
  SELECT key, min(bucket) AS first_bucket, sum(count) AS total
  FROM lighthouse_aggregates
  WHERE kind = 'issue' AND resolution = 3600
  GROUP BY key
) a
WHERE a.key = i.fingerprint;

COMMIT;
