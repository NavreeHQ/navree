package com.navree.client.platform

import android.webkit.CookieManager
import com.navree.client.core.Body
import com.navree.client.core.Effect
import com.navree.client.core.Events
import com.navree.client.core.Part
import kotlinx.serialization.json.JsonObject
import okhttp3.Call
import okhttp3.Callback
import okhttp3.ConnectionSpec
import okhttp3.Cookie
import okhttp3.CookieJar
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.Interceptor
import okhttp3.MediaType.Companion.toMediaTypeOrNull
import okhttp3.MultipartBody
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import okio.BufferedSource
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
 * The most bytes the body of one answer may have: 8 MiB. A page is the tree of
 * one screen, with long lists loaded a page at a time, so the largest answers
 * seen are tens of kilobytes; a body is held as bytes, as text and once more
 * inside the event to the core, so 8 MiB stays well inside the memory a phone
 * gives an app. A longer body is answered as a failed request.
 */
const val MOST_BODY_BYTES: Long = 8L * 1024 * 1024

/**
 * The connections the client makes: TLS 1.2 or 1.3 with the modern cipher
 * suites only, and clear text only where the app allows plain `http` to the
 * machine itself (each address is checked again by [Origins.ours] before it
 * is asked).
 */
fun connectionSpecs(loopbackHttp: Boolean): List<ConnectionSpec> =
    if (loopbackHttp) listOf(ConnectionSpec.MODERN_TLS, ConnectionSpec.CLEARTEXT) else listOf(ConnectionSpec.MODERN_TLS)

/**
 * The request a `fetch` effect asks for, or null when it cannot be sent: an
 * address that is not one of the app's origins, a header value HTTP cannot
 * carry (a line break, a character outside ASCII), a method that cannot have
 * the body it was given. The caller answers null as a failed request.
 */
fun fetchRequest(
    effect: Effect.Fetch,
    origins: Origins,
    cookies: CookieJar,
    files: (String) -> RequestBody? = { null },
): Request? = try {
    val url = effect.url.takeIf(origins::ours)?.toHttpUrlOrNull()
    if (url == null) {
        null
    } else {
        val builder = Request.Builder().url(url)
        for ((name, value) in effect.headers) builder.header(name, value)
        if (effect.xsrf) xsrfToken(cookies, url)?.let { builder.header("x-xsrf-token", it) }
        val method = effect.method.uppercase()
        val type = effect.headers.firstOrNull { it.first.equals("content-type", ignoreCase = true) }?.second
        val body: RequestBody? = when (val sent = effect.body) {
            is Body.Form -> sent.text.toRequestBody(type?.toMediaTypeOrNull())
            is Body.Json -> sent.text.toRequestBody(type?.toMediaTypeOrNull())
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
        builder.build()
    }
} catch (_: Exception) {
    null
}

/**
 * The bytes of a body, or null when it has more than [most]: read only as far
 * as one byte past the limit, so a body that never ends costs no more.
 */
fun readCapped(source: BufferedSource, most: Long): ByteArray? {
    if (source.request(most + 1)) return null
    return source.readByteArray()
}

/**
 * Refuses, before any connection is made, a request to an address that is
 * not one of the app's origins: the last word on every request the client's
 * own connections make, whoever built it.
 */
class OriginGuard(private val allowed: (String) -> Boolean) : Interceptor {
    override fun intercept(chain: Interceptor.Chain): Response {
        val url = chain.request().url.toString()
        if (!allowed(url)) throw IOException("not one of the app's origins")
        return chain.proceed(chain.request())
    }
}

/**
 * Carries out the core's `fetch` effects with OkHttp: exactly the headers it
 * gives, the cookie store's cookies, no redirect followed (a 3xx goes back to
 * the core as it came, after its cookies are stored), and the answer handed
 * back whatever its status, with the `x-navree-*` headers.
 *
 * The core decides which addresses are asked; the platform checks each again
 * ([Origins.ours]) and answers one it refuses, or a request that cannot be
 * built, as a failed request, the failure the core already understands.
 */
class CoreHttp(
    val cookies: CookieJar,
    private val origins: Origins,
    loopbackHttp: Boolean = false,
    private val files: (String) -> RequestBody? = { null },
    /** Where an answer goes: posted to the thread the core is driven on. */
    private val answer: (JsonObject) -> Unit,
) {
    val client: OkHttpClient = OkHttpClient.Builder()
        .cookieJar(cookies)
        .followRedirects(false)
        .followSslRedirects(false)
        .connectionSpecs(connectionSpecs(loopbackHttp))
        .addInterceptor(OriginGuard(origins::mayLoad))
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(60, TimeUnit.SECONDS)
        .build()

    private val calls = ConcurrentHashMap<Long, Call>()

    fun fetch(effect: Effect.Fetch) {
        val request = fetchRequest(effect, origins, cookies, files)
        val call = try {
            request?.let { client.newCall(it).apply { timeout().timeout(effect.timeoutMs, TimeUnit.MILLISECONDS) } }
        } catch (_: Exception) {
            null
        }
        if (call == null) {
            answer(Events.responseFailed(effect.request, "other"))
            return
        }
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
                    } catch (_: Exception) {
                        Events.responseFailed(effect.request, "other")
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
        val body = if (redirect) {
            ""
        } else {
            val bytes = if (response.body.contentLength() > MOST_BODY_BYTES) null else readCapped(response.body.source(), MOST_BODY_BYTES)
            bytes ?: return Events.responseFailed(request, "other")
            String(bytes, response.body.contentType()?.charset(Charsets.UTF_8) ?: Charsets.UTF_8)
        }
        return Events.response(
            request = request,
            status = response.code,
            contentType = response.header("Content-Type"),
            location = if (redirect) response.header("Location") else null,
            headers = headers,
            body = body,
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
