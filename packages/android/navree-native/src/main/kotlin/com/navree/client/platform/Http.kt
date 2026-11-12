package com.navree.client.platform

import android.webkit.CookieManager
import com.navree.client.core.Body
import com.navree.client.core.Effect
import com.navree.client.core.Events
import com.navree.client.core.Part
import kotlinx.serialization.json.JsonObject
import okhttp3.Call
import okhttp3.Callback
import okhttp3.Cookie
import okhttp3.CookieJar
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.MediaType.Companion.toMediaTypeOrNull
import okhttp3.MultipartBody
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import java.io.IOException
import java.io.InterruptedIOException
import java.net.ConnectException
import java.net.URLDecoder
import java.net.UnknownHostException
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit

/**
 * The session's cookies, kept in the system's own store for web views: the one
 * store the native requests and the web view both read and write, so a page
 * that falls back to the web view is signed in, and signing out clears both.
 */
class WebViewCookies(private val manager: () -> CookieManager = { CookieManager.getInstance() }) : CookieJar {
    override fun saveFromResponse(url: HttpUrl, cookies: List<Cookie>) {
        val store = manager()
        store.setAcceptCookie(true)
        for (cookie in cookies) store.setCookie(url.toString(), cookie.toString())
        store.flush()
    }

    override fun loadForRequest(url: HttpUrl): List<Cookie> {
        val header = manager().getCookie(url.toString()) ?: return emptyList()
        return header.split(';').mapNotNull { Cookie.parse(url, it.trim()) }
    }

    /** Every cookie of the store gone: the session ended on this device. */
    fun clear(done: () -> Unit = {}) {
        val store = manager()
        store.removeAllCookies { done() }
        store.flush()
    }
}

/**
 * The value an `x-xsrf-token` header carries: the `XSRF-TOKEN` cookie for the
 * address, percent-decoded. Null when there is none.
 */
fun xsrfToken(jar: CookieJar, url: HttpUrl): String? =
    jar.loadForRequest(url).firstOrNull { it.name == "XSRF-TOKEN" }?.value?.let {
        runCatching { URLDecoder.decode(it, "UTF-8") }.getOrDefault(it)
    }

/**
 * Carries out the core's `fetch` effects with OkHttp: exactly the headers it
 * gives, the cookie store's cookies, no redirect followed (a 3xx goes back to
 * the core as it came, after its cookies are stored), and the answer handed
 * back whatever its status, with the `x-navree-*` headers.
 *
 * Whether an address may be asked at all is the core's to decide; this sends
 * what it is told and nothing more.
 */
class CoreHttp(
    val cookies: CookieJar,
    private val files: (String) -> RequestBody? = { null },
    /** Where an answer goes: posted to the thread the core is driven on. */
    private val answer: (JsonObject) -> Unit,
) {
    val client: OkHttpClient = OkHttpClient.Builder()
        .cookieJar(cookies)
        .followRedirects(false)
        .followSslRedirects(false)
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(60, TimeUnit.SECONDS)
        .build()

    private val calls = ConcurrentHashMap<Long, Call>()

    fun fetch(effect: Effect.Fetch) {
        val url = effect.url.toHttpUrlOrNull()
        if (url == null) {
            answer(Events.responseFailed(effect.request, "other"))
            return
        }
        val builder = Request.Builder().url(url)
        for ((name, value) in effect.headers) builder.header(name, value)
        if (effect.xsrf) xsrfToken(cookies, url)?.let { builder.header("x-xsrf-token", it) }
        val method = effect.method.uppercase()
        val type = effect.headers.firstOrNull { it.first.equals("content-type", ignoreCase = true) }?.second
        val body: RequestBody? = when (val sent = effect.body) {
            is Body.Form -> sent.text.toRequestBody(type?.toMediaTypeOrNull())
            is Body.Multipart -> MultipartBody.Builder().setType(MultipartBody.FORM).apply {
                for (part in sent.parts) {
                    when (part) {
                        is Part.Text -> addFormDataPart(part.name, part.value)
                        is Part.File -> files(part.token)?.let { addFormDataPart(part.name, part.filename, it) }
                    }
                }
            }.build()
            null -> if (method == "GET" || method == "HEAD") null else ByteArray(0).toRequestBody(type?.toMediaTypeOrNull())
        }
        if (body is MultipartBody) builder.removeHeader("content-type")
        builder.method(method, body)
        val call = client.newCall(builder.build())
        call.timeout().timeout(effect.timeoutMs, TimeUnit.MILLISECONDS)
        calls[effect.request] = call
        call.enqueue(
            object : Callback {
                override fun onFailure(call: Call, e: IOException) {
                    calls.remove(effect.request)
                    if (call.isCanceled()) return
                    answer(Events.responseFailed(effect.request, failure(e)))
                }

                override fun onResponse(call: Call, response: Response) {
                    calls.remove(effect.request)
                    val event = try {
                        response.use { read(effect.request, it) }
                    } catch (e: IOException) {
                        if (call.isCanceled()) return
                        Events.responseFailed(effect.request, failure(e))
                    }
                    answer(event)
                }
            },
        )
    }

    fun cancel(request: Long) {
        calls.remove(request)?.cancel()
    }

    private fun read(request: Long, response: Response): JsonObject {
        val headers = response.headers.names()
            .filter { it.startsWith("x-navree-", ignoreCase = true) }
            .flatMap { name -> response.headers.values(name).map { name.lowercase() to it } }
        val redirect = response.code in REDIRECTS
        return Events.response(
            request = request,
            status = response.code,
            contentType = response.header("Content-Type"),
            location = if (redirect) response.header("Location") else null,
            headers = headers,
            body = if (redirect) "" else response.body.string(),
        )
    }

    private fun failure(e: IOException): String = when (e) {
        is UnknownHostException, is ConnectException -> "offline"
        is InterruptedIOException -> "timeout"
        else -> "other"
    }

    private companion object {
        val REDIRECTS = setOf(301, 302, 303, 307, 308)
    }
}
