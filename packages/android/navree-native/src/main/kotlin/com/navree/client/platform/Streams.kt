package com.navree.client.platform

import com.navree.client.core.Effect
import com.navree.client.core.Events
import kotlinx.serialization.json.JsonObject
import okhttp3.Call
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.IOException
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit
import kotlin.concurrent.thread

/**
 * The most bytes one line of an event stream may have: 256 KiB. The client
 * reads only an event's name, but each line is held whole while it is read;
 * the server's events are short notices that something changed, so a line
 * this long is not one of them, and the stream is ended (the core asks for it
 * again when it wants to).
 */
const val MOST_LINE_BYTES: Long = 256L * 1024

/**
 * The request a `stream_open` effect asks for, or null when it cannot be
 * made: an address that is not one of the app's origins, or a header value
 * HTTP cannot carry.
 */
fun streamRequest(effect: Effect.StreamOpen, origins: Origins): Request? = try {
    val url = effect.url.takeIf(origins::ours)?.toHttpUrlOrNull()
    if (url == null) {
        null
    } else {
        val builder = Request.Builder().url(url).header("Accept", "text/event-stream")
        for ((name, value) in effect.headers) builder.header(name, value)
        builder.build()
    }
} catch (_: Exception) {
    null
}

/**
 * Server-sent event streams the core asks for, with the session's cookies. It
 * reports the stream open, the name of each event, and its end with the HTTP
 * status. It never reads an event's data, and never reconnects by itself:
 * the core asks again when it wants to.
 */
class CoreStreams(base: OkHttpClient, private val origins: Origins, private val answer: (JsonObject) -> Unit) {
    // A stream may be quiet for as long as it likes.
    private val client = base.newBuilder().readTimeout(0, TimeUnit.MILLISECONDS).build()
    private val calls = ConcurrentHashMap<Long, Call>()

    fun open(effect: Effect.StreamOpen) {
        val call = try {
            streamRequest(effect, origins)?.let(client::newCall)
        } catch (_: Exception) {
            null
        }
        if (call == null) {
            answer(Events.streamClosed(effect.stream, 0))
            return
        }
        calls[effect.stream] = call
        thread(name = "navree-stream-${effect.stream}", isDaemon = true) { listen(effect.stream, call) }
    }

    fun close(stream: Long) {
        calls.remove(stream)?.cancel()
    }

    fun closeAll() {
        for (stream in calls.keys.toList()) close(stream)
    }

    private fun listen(stream: Long, call: Call) {
        var status = 0
        try {
            call.execute().use { response ->
                status = response.code
                if (response.code != 200) return@use
                answer(Events.streamOpen(stream))
                val source = response.body.source()
                var name: String? = null
                var data = false
                while (!call.isCanceled()) {
                    // A line with no end within the limit, or a stream that stops mid-line, ends the stream.
                    val line = source.readUtf8LineStrict(MOST_LINE_BYTES)
                    when {
                        line.isEmpty() -> {
                            // A blank line ends an event; one with no data is not dispatched.
                            if (data) answer(Events.streamEvent(stream, name))
                            name = null
                            data = false
                        }
                        line.startsWith(":") -> Unit
                        line.startsWith("event:") -> name = line.removePrefix("event:").trim().ifEmpty { null }
                        line.startsWith("data") -> data = true
                    }
                }
            }
        } catch (_: IOException) {
            // Ended or failed: said below, unless the core closed it.
        } catch (_: RuntimeException) {
            // Anything else a broken answer causes ends the stream the same way.
        }
        if (calls.remove(stream) != null) answer(Events.streamClosed(stream, status))
    }
}
