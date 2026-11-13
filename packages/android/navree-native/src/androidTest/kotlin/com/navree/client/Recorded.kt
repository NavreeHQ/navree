package com.navree.client

import androidx.test.platform.app.InstrumentationRegistry
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.RecordedRequest
import java.net.URLDecoder

/**
 * Serves answers recorded from a server, from the test's assets, by the
 * address asked: `/orders/1` is `orders_1.json`, `/orders?sort=total` is
 * `orders_sort_total.json`, `/` is `index.json`. A native request gets the
 * JSON; the web view's gets the page's HTML when it was recorded.
 */
class Recorded(
    private val folder: String,
    private val routes: Map<String, String> = emptyMap(),
    /** Reads an answer by its file name from somewhere else than the test's assets, when given. */
    private val read: ((String) -> String?)? = null,
) : Dispatcher() {
    private val assets = InstrumentationRegistry.getInstrumentation().context.assets

    /** Every address asked, in order, for the test to look at. */
    val asked = java.util.Collections.synchronizedList(ArrayList<String>())

    override fun dispatch(request: RecordedRequest): MockResponse {
        val url = request.url
        val path = url.encodedPath
        asked += request.method + " " + path + (url.encodedQuery?.let { "?$it" } ?: "")
        val native = request.headers["x-navree-native"] != null
        val name = routes[path] ?: nameOf(path, url.encodedQuery)
        val prefix = if (folder.isEmpty()) "" else "$folder/"
        val file = if (native) "$prefix$name.json" else "$prefix$name.embed.html"
        val body = read?.invoke(file) ?: runCatching { assets.open(file).use { it.readBytes().decodeToString() } }.getOrNull()
            ?: return MockResponse.Builder().code(404).addHeader("Content-Type", "text/plain").body("Not recorded: $file").build()
        return MockResponse.Builder()
            .code(200)
            .addHeader("Content-Type", if (native) "application/vnd.navree.native+json" else "text/html; charset=utf-8")
            .body(body)
            .build()
    }

    companion object {
        fun nameOf(path: String, query: String?): String {
            val base = path.trim('/').replace('/', '_').ifEmpty { "index" }
            val pairs = query?.split('&')?.filter { it.isNotEmpty() }?.joinToString("") { pair ->
                val (key, value) = pair.split('=', limit = 2).let { it[0] to it.getOrElse(1) { "" } }
                "_" + URLDecoder.decode(key, "UTF-8") + "_" + URLDecoder.decode(value, "UTF-8")
            }.orEmpty()
            return base + pairs
        }
    }
}
