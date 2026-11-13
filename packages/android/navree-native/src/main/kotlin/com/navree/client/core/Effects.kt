package com.navree.client.core

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.longOrNull

/** A part of a multipart body, in order. */
sealed interface Part {
    data class Text(val name: String, val value: String) : Part

    /** A file's bytes are the ones the client keeps under `token`. */
    data class File(val name: String, val token: String, val filename: String, val mediaType: String) : Part
}

/** What a request sends. */
sealed interface Body {
    /** Sent as the body bytes, even when empty; the content type is in the headers. */
    data class Form(val text: String) : Body

    data class Multipart(val parts: List<Part>) : Body

    /** JSON, sent as the body bytes; the content type is in the headers. */
    data class Json(val text: String) : Body
}

/**
 * What the core asks the client to do. Read from the core's JSON; an effect
 * this build does not know is [Unknown], logged and passed over.
 */
sealed interface Effect {
    data class Fetch(
        val request: Long,
        val method: String,
        val url: String,
        val headers: List<Pair<String, String>>,
        val body: Body?,
        val timeoutMs: Long,
        val xsrf: Boolean,
    ) : Effect

    data class Cancel(val request: Long) : Effect

    data class OpenUrl(val url: String, val inside: Boolean) : Effect

    data class SignIn(val url: String, val callback: String?, val ephemeral: Boolean) : Effect

    /** A message to show; the same list is `messages` in the view state. */
    data class Show(val id: Long) : Effect

    data class Haptic(val kind: String) : Effect

    /**
     * A device call. `device` is the protocol's call as JSON (`{"f":"share",...}`);
     * `options` what the server answered the core for it (a confirmation's passkey
     * request, a sign-in's nonce).
     */
    data class Device(val call: Long, val device: JsonObject, val posts: Boolean, val options: JsonObject?) : Effect

    data class CacheRead(val key: String) : Effect

    data class CacheWrite(val key: String, val value: String?) : Effect

    data object CacheClear : Effect

    data class Announce(val text: String?, val notice: Notice?, val count: Int?) : Effect

    data class Copy(val text: String, val sensitive: Boolean) : Effect

    data class Timer(val timer: Long, val ms: Long) : Effect

    data class CancelTimer(val timer: Long) : Effect

    data class StreamOpen(val stream: Long, val url: String, val headers: List<Pair<String, String>>) : Effect

    data class StreamClose(val stream: Long) : Effect

    data object Exit : Effect

    data class Report(val what: String, val detail: String) : Effect

    data class Unknown(val t: String) : Effect

    companion object {
        fun read(json: JsonElement): Effect {
            val o = json as? JsonObject ?: return Unknown("")
            fun s(name: String) = (o[name] as? JsonPrimitive)?.contentOrNull
            fun l(name: String) = (o[name] as? JsonPrimitive)?.longOrNull
            fun b(name: String) = (o[name] as? JsonPrimitive)?.booleanOrNull == true
            return when (val t = s("t")) {
                "fetch" -> Fetch(
                    request = l("request") ?: 0,
                    method = s("method") ?: "get",
                    url = s("url") ?: "",
                    headers = pairs(o["headers"]),
                    body = body(o["body"]),
                    timeoutMs = l("timeout_ms") ?: 30_000,
                    xsrf = b("xsrf"),
                )
                "cancel" -> Cancel(l("request") ?: 0)
                "open_url" -> OpenUrl(s("url") ?: "", b("inside"))
                "sign_in" -> SignIn(s("url") ?: "", s("callback"), b("ephemeral"))
                "message" -> Show(l("id") ?: 0)
                "haptic" -> Haptic(s("kind") ?: "light")
                "device" -> Device(l("call") ?: 0, o["device"] as? JsonObject ?: JsonObject(emptyMap()), b("posts"), o["options"] as? JsonObject)
                "cache_read" -> CacheRead(s("key") ?: "")
                "cache_write" -> CacheWrite(s("key") ?: "", s("value"))
                "cache_clear" -> CacheClear
                "announce" -> Announce(
                    text = s("text"),
                    notice = s("notice")?.let { word -> Notice.entries.firstOrNull { it.wire == word } },
                    count = l("count")?.toInt(),
                )
                "copy" -> Copy(s("text") ?: "", b("sensitive"))
                "timer" -> Timer(l("timer") ?: 0, l("ms") ?: 0)
                "cancel_timer" -> CancelTimer(l("timer") ?: 0)
                "stream_open" -> StreamOpen(l("stream") ?: 0, s("url") ?: "", pairs(o["headers"]))
                "stream_close" -> StreamClose(l("stream") ?: 0)
                "exit" -> Exit
                "report" -> Report(s("what") ?: "", s("detail") ?: "")
                else -> Unknown(t ?: "")
            }
        }

        private fun pairs(json: JsonElement?): List<Pair<String, String>> =
            (json as? JsonArray)?.mapNotNull { pair ->
                val items = pair as? JsonArray ?: return@mapNotNull null
                val name = (items.getOrNull(0) as? JsonPrimitive)?.contentOrNull ?: return@mapNotNull null
                val value = (items.getOrNull(1) as? JsonPrimitive)?.contentOrNull ?: ""
                name to value
            }.orEmpty()

        private fun body(json: JsonElement?): Body? {
            val o = json as? JsonObject ?: return null
            fun s(from: JsonObject, name: String) = (from[name] as? JsonPrimitive)?.contentOrNull
            return when (s(o, "t")) {
                "form" -> Body.Form(s(o, "text") ?: "")
                "json" -> Body.Json(s(o, "text") ?: "")
                "multipart" -> Body.Multipart(
                    (o["parts"] as? JsonArray)?.mapNotNull { element ->
                        val part = element as? JsonObject ?: return@mapNotNull null
                        when (s(part, "t")) {
                            "text" -> Part.Text(s(part, "name") ?: "", s(part, "value") ?: "")
                            "file" -> Part.File(s(part, "name") ?: "", s(part, "token") ?: "", s(part, "filename") ?: "", s(part, "type") ?: "application/octet-stream")
                            else -> null
                        }
                    }.orEmpty(),
                )
                else -> null
            }
        }
    }
}

/** The core's word for a notice. */
val Notice.wire: String
    get() = when (this) {
        Notice.Offline -> "offline"
        Notice.Timeout -> "timeout"
        Notice.Failed -> "failed"
        Notice.Unsaved -> "unsaved"
        Notice.Loading -> "loading"
        Notice.Copied -> "copied"
        Notice.FieldsNeedAttention -> "fields_need_attention"
        Notice.Loaded -> "loaded"
        Notice.SecondStep -> "second_step"
    }
