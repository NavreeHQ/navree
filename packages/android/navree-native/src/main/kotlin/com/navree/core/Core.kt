package com.navree.core

/**
 * The runtime core: everything a client does besides drawing, placing and
 * carrying out effects. JSON in, JSON out. Call it from one thread.
 *
 * Each call and [close] hold the same lock, so a call made while another
 * thread frees the core either finishes first or finds the core closed: the
 * native handle is never used after it was freed.
 *
 * ```
 * val core = Core(config)
 * val update = core.send("""{"t":"start"}""".toByteArray())
 * val nav = core.view("""{"part":"nav"}""".toByteArray())
 * core.close()
 * ```
 */
class Core(config: ByteArray) : AutoCloseable {
    private val lock = Any()
    private var handle: Long = nativeNew(config).also { require(it != 0L) { "the configuration does not read" } }

    /** Tells the core one thing that happened; answers what changed and what to do. */
    fun send(event: ByteArray): ByteArray = synchronized(lock) {
        check(handle != 0L) { "the core is closed" }
        nativeSend(handle, event)
    }

    /** Reads a part of the view state; an empty query reads all of it. */
    fun view(query: ByteArray = ByteArray(0)): ByteArray = synchronized(lock) {
        check(handle != 0L) { "the core is closed" }
        nativeView(handle, query)
    }

    override fun close() {
        synchronized(lock) {
            if (handle != 0L) {
                nativeFree(handle)
                handle = 0
            }
        }
    }

    private companion object {
        init {
            System.loadLibrary("navree_native_core")
        }

        @JvmStatic external fun nativeNew(config: ByteArray): Long

        @JvmStatic external fun nativeSend(handle: Long, event: ByteArray): ByteArray

        @JvmStatic external fun nativeView(handle: Long, query: ByteArray): ByteArray

        @JvmStatic external fun nativeFree(handle: Long)
    }
}
