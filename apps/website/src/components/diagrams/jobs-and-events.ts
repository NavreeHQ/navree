import type { DiagramSpec } from "./figure";

/**
 * From an emitted event or a dispatched job to a finished job. Wide: the
 * left column follows one job down from the request; sync listeners run
 * beside the unit of work (the accent), which is what makes a queued job
 * wait for the commit; the worker's three outcomes fan out below.
 * Narrow: the same steps as a column.
 */
export const jobsAndEvents: DiagramSpec = {
  title: "From an event or a job to a finished job",
  desc: "A request, command or job emits an event or dispatches a job. Sync listeners run at once, inside the request and before it commits. A queued listener's job and every dispatched job join the request's unit of work, so they are queued only if the request commits. After the commit they wait in the queue, whose driver is the database, memory or Redis. A worker started with queue:work takes the next job from the first queue that has one and runs its handler. A job that succeeds is finished. A failed attempt with attempts left goes back on the queue after its backoff delay. A job that has used up its attempts moves to the failed jobs, where queue:retry puts it back on its queue.",
  legend: [
    { kind: "flow", label: "in the request" },
    { kind: "async", label: "after the commit" },
  ],
  wide: {
    width: 592,
    height: 408,
    nodes: [
      { id: "emit", x: 0, y: 0, w: 352, title: "Emit an event or dispatch a job", sub: "in a request, a command or a job", icon: "event" },
      { id: "uow", x: 0, y: 88, w: 352, title: "Unit of work", sub: "queued jobs join the request's transaction", icon: "database", tone: "accent" },
      { id: "queue", x: 0, y: 176, w: 352, title: "Queue", sub: "database, memory or Redis", icon: "queue" },
      { id: "worker", x: 0, y: 264, w: 352, title: "Worker", sub: "queue:work runs the job's handler", icon: "worker" },
      { id: "sync", x: 416, y: 0, w: 176, title: "Sync listeners", sub: "run at once", icon: "code" },
      { id: "done", x: 0, y: 352, w: 176, title: "Done", sub: "the job is finished", align: "center" },
      { id: "retry", x: 208, y: 352, w: 176, title: "Retry", sub: "back after a backoff", align: "center" },
      { id: "failed", x: 416, y: 352, w: 176, title: "failed_jobs", mono: true, sub: "queue:retry puts it back", align: "center" },
    ],
    edges: [
      { from: "emit.bottom", to: "uow.top", step: 0 },
      { from: "emit.right", to: "sync.left", step: 0 },
      { from: "uow.bottom", to: "queue.top", kind: "async", step: 1, label: "after the commit", labelMono: false, at: [184, 164] },
      { from: "queue.bottom", to: "worker.top", kind: "async", step: 2 },
      { from: "worker.bottom", to: "done.top", kind: "async", bend: 336, step: 3 },
      { from: "worker.bottom", to: "retry.top", kind: "async", bend: 336, step: 3 },
      { from: "worker.bottom", to: "failed.top", kind: "async", bend: 336, step: 3 },
    ],
  },
  narrow: {
    width: 272,
    height: 656,
    nodes: [
      { id: "emit", x: 24, y: 0, w: 248, title: "Emit or dispatch", sub: "an event or a job", icon: "event" },
      { id: "sync", x: 24, y: 88, w: 248, title: "Sync listeners", sub: "run at once, before the commit", icon: "code" },
      { id: "uow", x: 24, y: 176, w: 248, title: "Unit of work", sub: "queued jobs wait for the commit", icon: "database", tone: "accent" },
      { id: "queue", x: 24, y: 264, w: 248, title: "Queue", sub: "database, memory or Redis", icon: "queue" },
      { id: "worker", x: 24, y: 352, w: 248, title: "Worker", sub: "queue:work runs the handler", icon: "worker" },
      { id: "done", x: 24, y: 440, w: 248, title: "Done", sub: "the job is finished", align: "center" },
      { id: "retry", x: 24, y: 520, w: 248, title: "Retry", sub: "back after a backoff", align: "center" },
      { id: "failed", x: 24, y: 600, w: 248, title: "failed_jobs", mono: true, sub: "queue:retry puts it back", align: "center" },
    ],
    edges: [
      { from: "emit.bottom", to: "sync.top", step: 0 },
      { from: "sync.bottom", to: "uow.top", step: 0 },
      { from: "uow.bottom", to: "queue.top", kind: "async", step: 1 },
      { from: "queue.bottom", to: "worker.top", kind: "async", step: 2 },
      { from: "worker.left", to: "done.left", kind: "async", bend: 8, step: 3 },
      { from: "worker.left", to: "retry.left", kind: "async", bend: 8, step: 3 },
      { from: "worker.left", to: "failed.left", kind: "async", bend: 8, step: 3 },
    ],
  },
};
