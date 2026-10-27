//! [`RedisQueueStore`]: navree-queue's jobs on Redis.

use std::fmt;

use navree_contracts::BoxFuture;
use navree_queue::{FailedJob, NewJob, QueueError, QueueStore, ReservedJob, RetryTarget};
use redis::Script;
use redis::aio::ConnectionManager;

use crate::RedisConnection;

/// Stores a new job; a pending job with the same unique key that still
/// blocks it wins (its id is returned).
///
/// KEYS: job hash, pending set. ARGV: id, body, queue, available_at,
/// created_at, stale_ms, unique key ("" when none), unique window in ms
/// (-1: while pending), the job hash prefix, the unique pointer prefix.
const INSERT: &str = r"
local id = ARGV[1]
local unique = ARGV[7]
if unique ~= '' then
  local pointer = ARGV[10] .. unique
  local pending = redis.call('GET', pointer)
  if pending then
    local pending_key = ARGV[9] .. pending
    local created = redis.call('HGET', pending_key, 'created_at')
    if created then
      local window = tonumber(ARGV[8])
      if window < 0 or tonumber(ARGV[5]) < tonumber(created) + window then
        return pending
      end
      redis.call('HSET', pending_key, 'unique_key', '')
    end
  end
  redis.call('SET', pointer, id)
end
redis.call('HSET', KEYS[1],
  'body', ARGV[2], 'queue', ARGV[3], 'attempts', 0,
  'available_at', ARGV[4], 'created_at', ARGV[5], 'stale_ms', ARGV[6],
  'unique_key', unique)
redis.call('ZADD', KEYS[2], ARGV[4], id)
return id
";

/// Reserves the next job of a queue: the smallest (available_at, id) among
/// the due pending jobs and the reservations that went stale.
///
/// KEYS: pending set, reserved set. ARGV: now, worker, the job hash prefix.
/// Returns {body, attempts, id} or nil.
const RESERVE: &str = r"
local now = tonumber(ARGV[1])
for _ = 1, 100 do
  local ready = redis.call('ZRANGEBYSCORE', KEYS[1], '-inf', now, 'WITHSCORES', 'LIMIT', 0, 1)
  local stale = redis.call('ZRANGEBYSCORE', KEYS[2], '-inf', now, 'LIMIT', 0, 1)
  local id = nil
  local from_pending = false
  if #ready > 0 then
    id = ready[1]
    from_pending = true
  end
  if #stale > 0 then
    local stale_at = tonumber(redis.call('HGET', ARGV[3] .. stale[1], 'available_at'))
    if id == nil or stale_at == nil or stale_at < tonumber(ready[2])
      or (stale_at == tonumber(ready[2]) and stale[1] < id) then
      id = stale[1]
      from_pending = false
    end
  end
  if id == nil then
    return nil
  end
  local key = ARGV[3] .. id
  if redis.call('EXISTS', key) == 0 then
    redis.call('ZREM', KEYS[1], id)
    redis.call('ZREM', KEYS[2], id)
  else
    if from_pending then
      redis.call('ZREM', KEYS[1], id)
    end
    local stale_ms = tonumber(redis.call('HGET', key, 'stale_ms'))
    redis.call('ZADD', KEYS[2], now + stale_ms, id)
    local attempts = redis.call('HINCRBY', key, 'attempts', 1)
    redis.call('HSET', key, 'reserved_by', ARGV[2], 'reserved_at', ARGV[1])
    return {redis.call('HGET', key, 'body'), attempts, id}
  end
end
return nil
";

/// Deletes a finished job and its unique pointer.
///
/// KEYS: job hash, pending set, reserved set. ARGV: id, unique pointer prefix.
const COMPLETE: &str = r"
local unique = redis.call('HGET', KEYS[1], 'unique_key')
if unique and unique ~= '' then
  local pointer = ARGV[2] .. unique
  if redis.call('GET', pointer) == ARGV[1] then
    redis.call('DEL', pointer)
  end
end
redis.call('DEL', KEYS[1])
redis.call('ZREM', KEYS[2], ARGV[1])
redis.call('ZREM', KEYS[3], ARGV[1])
return 1
";

/// Puts a reserved job back, available at a later time, if the worker
/// still holds it.
///
/// KEYS: job hash, pending set, reserved set. ARGV: id, worker, available_at.
const RELEASE: &str = r"
if redis.call('HGET', KEYS[1], 'reserved_by') ~= ARGV[2] then
  return 0
end
redis.call('HDEL', KEYS[1], 'reserved_by', 'reserved_at')
redis.call('HSET', KEYS[1], 'available_at', ARGV[3])
redis.call('ZREM', KEYS[3], ARGV[1])
redis.call('ZADD', KEYS[2], ARGV[3], ARGV[1])
return 1
";

/// Moves a reserved job to the failed jobs, if the worker still holds it.
///
/// KEYS: job hash, reserved set, failed hash, failed index. ARGV: id,
/// worker, failed job JSON, failed_at, unique pointer prefix.
const FAIL: &str = r"
if redis.call('HGET', KEYS[1], 'reserved_by') ~= ARGV[2] then
  return 0
end
local unique = redis.call('HGET', KEYS[1], 'unique_key')
if unique and unique ~= '' then
  local pointer = ARGV[5] .. unique
  if redis.call('GET', pointer) == ARGV[1] then
    redis.call('DEL', pointer)
  end
end
redis.call('DEL', KEYS[1])
redis.call('ZREM', KEYS[2], ARGV[1])
redis.call('HSET', KEYS[3], ARGV[1], ARGV[3])
redis.call('ZADD', KEYS[4], ARGV[4], ARGV[1])
return 1
";

/// Moves one failed job back to its queue, if it is still failed.
///
/// KEYS: failed hash, failed index, job hash, pending set. ARGV: id, body,
/// queue, now, stale_ms.
const REQUEUE: &str = r"
if redis.call('HDEL', KEYS[1], ARGV[1]) == 0 then
  return 0
end
redis.call('ZREM', KEYS[2], ARGV[1])
redis.call('HSET', KEYS[3],
  'body', ARGV[2], 'queue', ARGV[3], 'attempts', 0,
  'available_at', ARGV[4], 'created_at', ARGV[4], 'stale_ms', ARGV[5],
  'unique_key', '')
redis.call('ZADD', KEYS[4], ARGV[4], ARGV[1])
return 1
";

/// Deletes every failed job; returns how many.
///
/// KEYS: failed hash, failed index.
const FLUSH_FAILED: &str = r"
local count = redis.call('HLEN', KEYS[1])
redis.call('DEL', KEYS[1], KEYS[2])
return count
";

/// navree-queue's [`QueueStore`] on Redis (`queue.driver = "redis"`).
///
/// Under `<prefix>queue:`:
///
/// - `job:<id>`: a hash per job: the job as JSON (`body`, never rewritten
///   by Redis), its queue, attempts, times and reservation;
/// - `pending:<queue>`: a sorted set of the waiting and delayed jobs,
///   scored by when they become available (ties in id order, which is
///   dispatch order);
/// - `reserved:<queue>`: a sorted set of the reserved jobs, scored by when
///   their reservation goes stale (the job's timeout plus a grace period),
///   after which another worker takes the job over;
/// - `unique:<key>`: the id of the pending job holding a unique key;
/// - `failed` and `failed:index`: the failed jobs as JSON, and their ids by
///   failure time.
///
/// Every move between them is one Lua script, so reservations are atomic
/// and a job is never reserved by two workers at once. Delivery is at
/// least once: a worker that dies mid-job leaves a reservation that goes
/// stale and is retried. Times come from the app's clock, not Redis's.
#[derive(Clone)]
pub struct RedisQueueStore {
    connection: RedisConnection,
    namespace: String,
    scripts: Scripts,
}

#[derive(Clone)]
struct Scripts {
    insert: Script,
    reserve: Script,
    complete: Script,
    release: Script,
    fail: Script,
    requeue: Script,
    flush_failed: Script,
}

impl RedisQueueStore {
    /// The store on `connection`, with keys under `<prefix>queue:`.
    pub fn new(connection: RedisConnection, prefix: &str) -> Self {
        Self {
            connection,
            namespace: format!("{prefix}queue:"),
            scripts: Scripts {
                insert: Script::new(INSERT),
                reserve: Script::new(RESERVE),
                complete: Script::new(COMPLETE),
                release: Script::new(RELEASE),
                fail: Script::new(FAIL),
                requeue: Script::new(REQUEUE),
                flush_failed: Script::new(FLUSH_FAILED),
            },
        }
    }

    fn manager(&self) -> ConnectionManager {
        self.connection.manager()
    }

    fn job_prefix(&self) -> String {
        format!("{}job:", self.namespace)
    }

    fn job_key(&self, id: &str) -> String {
        format!("{}job:{id}", self.namespace)
    }

    fn pending_key(&self, queue: &str) -> String {
        format!("{}pending:{queue}", self.namespace)
    }

    fn reserved_key(&self, queue: &str) -> String {
        format!("{}reserved:{queue}", self.namespace)
    }

    fn unique_prefix(&self) -> String {
        format!("{}unique:", self.namespace)
    }

    fn failed_key(&self) -> String {
        format!("{}failed", self.namespace)
    }

    fn failed_index_key(&self) -> String {
        format!("{}failed:index", self.namespace)
    }

    fn fail_with(&self, source: redis::RedisError) -> QueueError {
        QueueError::Store(Box::new(self.connection.error(source)))
    }

    /// Puts the failed job `failed` back on its queue at `now`; `false`
    /// when it is no longer a failed job.
    async fn requeue(&self, failed: &FailedJob, now: i64) -> Result<bool, QueueError> {
        let job = failed.requeued(now);
        let body = serde_json::to_string(&job).map_err(QueueError::Payload)?;
        let moved: i64 = self
            .scripts
            .requeue
            .key(self.failed_key())
            .key(self.failed_index_key())
            .key(self.job_key(&job.id))
            .key(self.pending_key(&job.queue))
            .arg(&job.id)
            .arg(body)
            .arg(&job.queue)
            .arg(now)
            .arg(job.stale_after_ms())
            .invoke_async(&mut self.manager())
            .await
            .map_err(|e| self.fail_with(e))?;
        Ok(moved == 1)
    }

    /// The failed jobs with these ids (missing ones skipped).
    async fn failed_jobs(&self, ids: &[String]) -> Result<Vec<FailedJob>, QueueError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let stored: Vec<Option<String>> = redis::cmd("HMGET")
            .arg(self.failed_key())
            .arg(ids)
            .query_async(&mut self.manager())
            .await
            .map_err(|e| self.fail_with(e))?;
        stored
            .into_iter()
            .flatten()
            .map(|json| serde_json::from_str(&json).map_err(QueueError::Payload))
            .collect()
    }
}

impl fmt::Debug for RedisQueueStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisQueueStore")
            .field("connection", &self.connection.name())
            .field("namespace", &self.namespace)
            .finish_non_exhaustive()
    }
}

impl QueueStore for RedisQueueStore {
    fn insert(&self, job: NewJob) -> BoxFuture<'_, Result<String, QueueError>> {
        Box::pin(async move {
            let body = serde_json::to_string(&job).map_err(QueueError::Payload)?;
            let id: String = self
                .scripts
                .insert
                .key(self.job_key(&job.id))
                .key(self.pending_key(&job.queue))
                .arg(&job.id)
                .arg(body)
                .arg(&job.queue)
                .arg(job.available_at)
                .arg(job.created_at)
                .arg(job.stale_after_ms())
                .arg(job.unique_key.as_deref().unwrap_or_default())
                .arg(job.unique_for_ms.unwrap_or(-1))
                .arg(self.job_prefix())
                .arg(self.unique_prefix())
                .invoke_async(&mut self.manager())
                .await
                .map_err(|e| self.fail_with(e))?;
            Ok(id)
        })
    }

    fn reserve<'a>(
        &'a self,
        queue: &'a str,
        worker: &'a str,
        now: i64,
    ) -> BoxFuture<'a, Result<Option<ReservedJob>, QueueError>> {
        Box::pin(async move {
            let reserved: Option<(String, u32, String)> = self
                .scripts
                .reserve
                .key(self.pending_key(queue))
                .key(self.reserved_key(queue))
                .arg(now)
                .arg(worker)
                .arg(self.job_prefix())
                .invoke_async(&mut self.manager())
                .await
                .map_err(|e| self.fail_with(e))?;
            let Some((body, attempts, id)) = reserved else {
                return Ok(None);
            };
            let job: NewJob = serde_json::from_str(&body).map_err(QueueError::Payload)?;
            Ok(Some(ReservedJob {
                id,
                queue: job.queue,
                job_type: job.job_type,
                payload: job.payload,
                options: job.options,
                attempts,
                trace_parent: job.trace_parent,
                membership: job.membership,
            }))
        })
    }

    fn complete<'a>(&'a self, job: &'a ReservedJob) -> BoxFuture<'a, Result<(), QueueError>> {
        Box::pin(async move {
            let _: i64 = self
                .scripts
                .complete
                .key(self.job_key(&job.id))
                .key(self.pending_key(&job.queue))
                .key(self.reserved_key(&job.queue))
                .arg(&job.id)
                .arg(self.unique_prefix())
                .invoke_async(&mut self.manager())
                .await
                .map_err(|e| self.fail_with(e))?;
            Ok(())
        })
    }

    fn release<'a>(
        &'a self,
        job: &'a ReservedJob,
        worker: &'a str,
        available_at: i64,
    ) -> BoxFuture<'a, Result<bool, QueueError>> {
        Box::pin(async move {
            let released: i64 = self
                .scripts
                .release
                .key(self.job_key(&job.id))
                .key(self.pending_key(&job.queue))
                .key(self.reserved_key(&job.queue))
                .arg(&job.id)
                .arg(worker)
                .arg(available_at)
                .invoke_async(&mut self.manager())
                .await
                .map_err(|e| self.fail_with(e))?;
            Ok(released == 1)
        })
    }

    fn fail<'a>(
        &'a self,
        job: &'a ReservedJob,
        worker: &'a str,
        error: &'a str,
        now: i64,
    ) -> BoxFuture<'a, Result<bool, QueueError>> {
        Box::pin(async move {
            let failed = FailedJob {
                id: job.id.clone(),
                queue: job.queue.clone(),
                job_type: job.job_type.clone(),
                payload: job.payload.clone(),
                options: job.options.clone(),
                error: error.to_owned(),
                attempts: job.attempts,
                failed_at: chrono::DateTime::from_timestamp_millis(now).unwrap_or_default(),
            };
            let json = serde_json::to_string(&failed).map_err(QueueError::Payload)?;
            let moved: i64 = self
                .scripts
                .fail
                .key(self.job_key(&job.id))
                .key(self.reserved_key(&job.queue))
                .key(self.failed_key())
                .key(self.failed_index_key())
                .arg(&job.id)
                .arg(worker)
                .arg(json)
                .arg(now)
                .arg(self.unique_prefix())
                .invoke_async(&mut self.manager())
                .await
                .map_err(|e| self.fail_with(e))?;
            Ok(moved == 1)
        })
    }

    fn failed(&self) -> BoxFuture<'_, Result<Vec<FailedJob>, QueueError>> {
        Box::pin(async move {
            // Newest first: by failure time, then id, both descending.
            let ids: Vec<String> = redis::cmd("ZREVRANGE")
                .arg(self.failed_index_key())
                .arg(0)
                .arg(-1)
                .query_async(&mut self.manager())
                .await
                .map_err(|e| self.fail_with(e))?;
            self.failed_jobs(&ids).await
        })
    }

    fn retry<'a>(
        &'a self,
        target: &'a RetryTarget,
        now: i64,
    ) -> BoxFuture<'a, Result<u64, QueueError>> {
        Box::pin(async move {
            let ids: Vec<String> = match target {
                RetryTarget::Id(id) => vec![id.clone()],
                // Oldest first, like the database driver.
                RetryTarget::All => redis::cmd("ZRANGE")
                    .arg(self.failed_index_key())
                    .arg(0)
                    .arg(-1)
                    .query_async(&mut self.manager())
                    .await
                    .map_err(|e| self.fail_with(e))?,
            };
            let mut retried = 0;
            for failed in self.failed_jobs(&ids).await? {
                if self.requeue(&failed, now).await? {
                    retried += 1;
                }
            }
            Ok(retried)
        })
    }

    fn flush_failed(&self) -> BoxFuture<'_, Result<u64, QueueError>> {
        Box::pin(async move {
            self.scripts
                .flush_failed
                .key(self.failed_key())
                .key(self.failed_index_key())
                .invoke_async(&mut self.manager())
                .await
                .map_err(|e| self.fail_with(e))
        })
    }

    fn pending_count<'a>(&'a self, queue: &'a str) -> BoxFuture<'a, Result<u64, QueueError>> {
        Box::pin(async move {
            let (pending, reserved): (u64, u64) = redis::pipe()
                .zcard(self.pending_key(queue))
                .zcard(self.reserved_key(queue))
                .query_async(&mut self.manager())
                .await
                .map_err(|e| self.fail_with(e))?;
            Ok(pending + reserved)
        })
    }
}
