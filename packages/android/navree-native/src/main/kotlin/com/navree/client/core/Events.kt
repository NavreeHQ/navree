package com.navree.client.core

import com.navree.protocol.Navree
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/**
 * The events a client tells the core, one a thing that happened. Each is the
 * JSON the core reads; the names and fields are the core's.
 */
object Events {
    val start: JsonObject = event("start")
    val back: JsonObject = event("back")
    val foreground: JsonObject = event("foreground")
    val background: JsonObject = event("background")
    val signedOut: JsonObject = event("signed_out")
    val openUpgrade: JsonObject = event("open_upgrade")

    fun select(destination: Int) = event("select") { put("destination", destination) }

    /**
     * Something with an action was tapped. `form` and `overlay` are the ids of
     * the nearest form and sheet, dialog or popover around it.
     */
    fun act(
        screen: Long?,
        action: Navree.Action,
        form: String? = null,
        overlay: String? = null,
        text: String? = null,
        now: Long = System.currentTimeMillis() / 1000,
    ) = event("act") {
        screen?.let { put("screen", it) }
        put("action", CoreJson.encodeToJsonElement(Navree.ActionSerializer, action))
        form?.let { put("form", it) }
        overlay?.let { put("overlay", it) }
        text?.let { put("text", it) }
        put("now", now)
    }

    fun field(screen: Long, form: String?, field: String, value: FieldValue) = event("field") {
        put("screen", screen)
        form?.let { put("form", it) }
        put("field", field)
        put("value", value.json())
    }

    fun search(screen: Long, text: String) = event("search") {
        put("screen", screen)
        put("text", text)
    }

    fun popTo(screen: Long) = event("pop_to") { put("screen", screen) }

    fun dismissed(screen: Long) = event("dismissed") { put("screen", screen) }

    fun overlay(screen: Long, id: String, open: Boolean) = event("overlay") {
        put("screen", screen)
        put("id", id)
        put("open", open)
    }

    fun refresh(screen: Long) = event("refresh") { put("screen", screen) }

    fun regionRetry(screen: Long, region: String) = event("region_retry") {
        put("screen", screen)
        put("region", region)
    }

    fun more(screen: Long, id: String) = event("more") {
        put("screen", screen)
        put("id", id)
    }

    fun regionGo(screen: Long, region: String, href: String) = event("region_go") {
        put("screen", screen)
        put("region", region)
        put("href", href)
    }

    fun messageShown(id: Long) = event("message_shown") { put("id", id) }

    fun messageAction(id: Long) = event("message_action") { put("id", id) }

    fun webLink(screen: Long, url: String) = event("web_link") {
        put("screen", screen)
        put("url", url)
    }

    fun openLink(url: String) = event("open_link") { put("url", url) }

    fun notification(href: String) = event("notification") { put("href", href) }

    fun window(width: Width) = event("window") { put("width", width.wire) }

    fun environment(
        dark: Boolean? = null,
        contrast: String? = null,
        textPercent: Int? = null,
        locale: String? = null,
        tz: String? = null,
    ) = event("environment") {
        dark?.let { put("scheme", if (it) "dark" else "light") }
        contrast?.let { put("contrast", it) }
        textPercent?.let { put("text_percent", it) }
        locale?.let { put("locale", it) }
        tz?.let { put("tz", it) }
    }

    fun timer(timer: Long) = event("timer") { put("timer", timer) }

    fun streamOpen(stream: Long) = event("stream") {
        put("stream", stream)
        put("state", "open")
    }

    fun streamEvent(stream: Long, name: String?) = event("stream") {
        put("stream", stream)
        put("state", "event")
        name?.let { put("name", it) }
    }

    fun streamClosed(stream: Long, status: Int) = event("stream") {
        put("stream", stream)
        put("state", "closed")
        put("status", status)
    }

    fun cache(key: String, value: String?) = event("cache") {
        put("key", key)
        value?.let { put("value", it) }
    }

    /** A device call that posts its result finished. Values are text, as the protocol has them. */
    fun deviceDone(call: Long, fields: List<Pair<String, String>>) = event("device_result") {
        put("call", call)
        put("ok", true)
        put(
            "fields",
            buildJsonArray {
                for ((name, value) in fields) {
                    add(
                        buildJsonObject {
                            put("name", name)
                            put("value", value)
                        },
                    )
                }
            },
        )
    }

    /** A device call that posts its result could not be done: one of the protocol's refusals. */
    fun deviceRefused(call: Long, refusal: String) = event("device_result") {
        put("call", call)
        put("ok", false)
        put("refusal", refusal)
    }

    fun signInDone(url: String?) = event("sign_in_done") { url?.let { put("url", it) } }

    fun focused(screen: Long, serial: Long) = event("focused") {
        put("screen", screen)
        put("serial", serial)
    }

    /** The answer to a fetch, whatever its status. Redirects are handed over as they came. */
    fun response(
        request: Long,
        status: Int,
        contentType: String?,
        location: String?,
        headers: List<Pair<String, String>>,
        body: String,
    ) = event("response") {
        put("request", request)
        put("status", status)
        contentType?.let { put("content_type", it) }
        location?.let { put("location", it) }
        if (headers.isNotEmpty()) put("headers", pairs(headers))
        if (body.isNotEmpty()) put("body", body)
    }

    /** A fetch that got no answer: `offline`, `timeout` or `other`. */
    fun responseFailed(request: Long, error: String) = event("response") {
        put("request", request)
        put("error", error)
    }

    private fun pairs(list: List<Pair<String, String>>): JsonArray = buildJsonArray {
        for ((name, value) in list) add(buildJsonArray { add(JsonPrimitive(name)); add(JsonPrimitive(value)) })
    }

    private fun event(t: String, body: kotlinx.serialization.json.JsonObjectBuilder.() -> Unit = {}): JsonObject = buildJsonObject {
        put("t", t)
        body()
    }
}

/** The word the core reads for a width. */
val Width.wire: String
    get() = when (this) {
        Width.Compact -> "compact"
        Width.Medium -> "medium"
        Width.Expanded -> "expanded"
    }
