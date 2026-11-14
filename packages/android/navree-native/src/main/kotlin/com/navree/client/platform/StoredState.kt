package com.navree.client.platform

import java.util.concurrent.atomic.AtomicBoolean

/**
 * The order of everything that touches what the app keeps on the device for
 * a person: the sealed page cache, the snapshot of a launch, the cookie store
 * and the wipe at the end of a session. Each piece of work runs on [executor], one
 * at a time, in the order it was asked for; a piece may finish later than it
 * returns (the wipe waits for the cookie store) and the next one waits for it.
 *
 * Each session has a number, [generation], which grows when the session ends
 * ([end]). A write carries the number of the session it belongs to and is
 * dropped if that session has ended by the time its turn comes; a read is
 * told whether its session is still the one on the device. So nothing a late
 * answer or a slow thread of the session before writes survives the wipe,
 * and nothing it reads is handed to the session after.
 */
/**
 * Runs the work of the end of a session so that it always lets the queue go
 * on: [arm] sets the time limit before anything else runs (it is given
 * [done] to call when the limit is reached), and a failure of any kind in
 * [work] calls [done] at once. [done] may then be called more than once;
 * [StoredState] takes only the first.
 */
fun endWithLimit(done: () -> Unit, arm: (release: () -> Unit) -> Unit, work: (done: () -> Unit) -> Unit) {
    arm(done)
    try {
        work(done)
    } catch (_: Throwable) {
        done()
    }
}

class StoredState(private val executor: (Runnable) -> Unit) {
    private val lock = Any()
    private val waiting = ArrayDeque<(() -> Unit) -> Unit>()
    private var busy = false

    /** The number of the session on the device now. */
    @Volatile
    var generation: Long = 0
        private set

    /** Work that runs in its turn whatever session it belongs to: a clear, a request's cookies read before it is sent. */
    fun always(work: () -> Unit) = enqueue { done ->
        try {
            work()
        } finally {
            done()
        }
    }

    /** A write for the session numbered [session]: dropped when that session has ended before its turn. */
    fun write(session: Long = generation, work: () -> Unit) = enqueue { done ->
        try {
            if (session == generation) work()
        } finally {
            done()
        }
    }

    /** A read for the session numbered [session]; [work] is told whether that session is still the one on the device. */
    fun read(session: Long = generation, work: (current: Boolean) -> Unit) = enqueue { done ->
        try {
            work(session == generation)
        } finally {
            done()
        }
    }

    /**
     * The session ended: from now on its writes are dropped. [work] (the wipe)
     * runs after what was asked before it, and calls `done` when everything
     * it started has finished; nothing after it runs before.
     */
    fun end(work: (done: () -> Unit) -> Unit) {
        synchronized(lock) { generation++ }
        enqueue(work)
    }

    private fun enqueue(task: (() -> Unit) -> Unit) {
        synchronized(lock) {
            waiting.addLast(task)
            if (busy) return
            busy = true
        }
        next()
    }

    private fun next() {
        val task = synchronized(lock) {
            val first = waiting.removeFirstOrNull()
            if (first == null) busy = false
            first
        } ?: return
        executor(
            Runnable {
                val finished = AtomicBoolean(false)
                val done = { if (finished.compareAndSet(false, true)) next() }
                try {
                    task(done)
                } catch (_: Throwable) {
                    // A failure of any kind releases the queue.
                    done()
                }
            },
        )
    }
}
