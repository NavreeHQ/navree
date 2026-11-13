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
 * Server-sent event streams the core asks for, with the session's cookies. It
 * reports the stream open, the name of each event, and its end with the HTTP
 * status. It never reads an event's data, and never reconnects by itself:
 * the core asks again when it wants to.
 */
class CoreStreams(base: OkHttpClient, private val answer: (JsonObject) -> Unit) {
    // A stream may be quiet for as long as it likes.
    private val client = base.newBuilder().readTimeout(0, TimeUnit.MILLISECONDS).build()
    private val calls = ConcurrentHashMap<Long, Call>()

    fun open(effect: Effect.StreamOpen) {
        val url = effect.url.toHttpUrlOrNull()
        if (url == null) {
            answer(Events.streamClosed(effect.stream, 0))
            return
        }
        val builder = Request.Builder().url(url).header("Accept", "text/event-stream")
        for ((name, value) in effect.headers) builder.header(name, value)
        val call = client.newCall(builder.build())
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
                    val line = source.readUtf8Line() ?: break
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
        }
        if (calls.remove(stream) != null) answer(Events.streamClosed(stream, status))
    }
}
