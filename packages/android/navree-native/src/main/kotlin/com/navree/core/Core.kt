package com.navree.core

/**
 * The runtime core: everything a client does besides drawing, placing and
 * carrying out effects. JSON in, JSON out. Call it from one thread.
 *
 * ```
 * val core = Core(config)
 * val update = core.send("""{"t":"start"}""".toByteArray())
 * val nav = core.view("""{"part":"nav"}""".toByteArray())
 * core.close()
 * ```
 */
class Core(config: ByteArray) : AutoCloseable {
    private var handle: Long = nativeNew(config).also { require(it != 0L) { "the configuration does not read" } }

    /** Tells the core one thing that happened; answers what changed and what to do. */
    fun send(event: ByteArray): ByteArray {
        check(handle != 0L) { "the core is closed" }
        return nativeSend(handle, event)
    }

    /** Reads a part of the view state; an empty query reads all of it. */
    fun view(query: ByteArray = ByteArray(0)): ByteArray {
        check(handle != 0L) { "the core is closed" }
        return nativeView(handle, query)
    }

    override fun close() {
        if (handle != 0L) {
            nativeFree(handle)
            handle = 0
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
