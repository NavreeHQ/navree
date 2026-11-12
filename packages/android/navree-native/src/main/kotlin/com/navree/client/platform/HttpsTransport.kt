package com.navree.client.platform

import android.webkit.CookieManager
import com.navree.client.runtime.ClientDescriptor
import com.navree.client.runtime.Failure
import com.navree.client.runtime.HttpRequest
import com.navree.client.runtime.HttpResponse
import com.navree.client.runtime.Origin
import com.navree.client.runtime.Target
import com.navree.client.runtime.Transport
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.Cookie
import okhttp3.CookieJar
import okhttp3.HttpUrl
import okhttp3.MediaType.Companion.toMediaTypeOrNull
import okhttp3.OkHttpClient
import okhttp3.RequestBody
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.IOException
import java.util.concurrent.TimeUnit

/**
 * The session's cookies, kept in the system's own store for web views. The
 * web view and the native requests read and write the same cookies, so a page
 * that falls back to the web view is signed in.
 */
class WebViewCookies : CookieJar {
    private val manager: CookieManager by lazy {
        CookieManager.getInstance().also { it.setAcceptCookie(true) }
    }

    override fun saveFromResponse(url: HttpUrl, cookies: List<Cookie>) {
        for (cookie in cookies) manager.setCookie(url.toString(), cookie.toString())
        manager.flush()
    }

    override fun loadForRequest(url: HttpUrl): List<Cookie> {
        val header = manager.getCookie(url.toString()) ?: return emptyList()
        return header.split(';').mapNotNull { Cookie.parse(url, it.trim()) }
    }
}

/**
 * Requests over HTTPS to the app's origin, and to nothing else. Redirects are
 * followed by hand so that one which leaves the origin is reported instead of
 * followed.
 */
class HttpsTransport(
    private val origin: Origin,
    private val describe: () -> ClientDescriptor,
    cookies: CookieJar = WebViewCookies(),
) : Transport {
    private val client: OkHttpClient = OkHttpClient.Builder()
        .cookieJar(cookies)
        .followRedirects(false)
        .followSslRedirects(false)
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .build()

    override suspend fun send(request: HttpRequest): HttpResponse = withContext(Dispatchers.IO) {
        try {
            follow(request)
        } catch (_: IOException) {
            HttpResponse.Failed(Failure.Offline)
        } catch (_: IllegalArgumentException) {
            HttpResponse.Failed(Failure.Protocol)
        }
    }

    /** Fetches an image from the origin. Null when it is elsewhere or cannot be had. */
    suspend fun bytes(url: String): ByteArray? = withContext(Dispatchers.IO) {
        val target = origin.resolve(url) as? Target.Inside ?: return@withContext null
        try {
            val call = okhttp3.Request.Builder().url(origin.absolute(target.url)).build()
            client.newCall(call).execute().use { response ->
                if (response.isSuccessful) response.body.bytes() else null
            }
        } catch (_: IOException) {
            null
        }
    }

    private fun follow(first: HttpRequest): HttpResponse {
        var method = first.method
        var url = first.url
        var body: RequestBody? = first.body?.let { it.bytes.toRequestBody(it.contentType.toMediaTypeOrNull()) }
        repeat(MOST_REDIRECTS) {
            val builder = okhttp3.Request.Builder().url(origin.absolute(url))
            for ((name, value) in describe().headers()) builder.header(name, value)
            val sends = method != "GET" && method != "HEAD"
            builder.method(method, if (sends) body ?: ByteArray(0).toRequestBody(null) else null)
            client.newCall(builder.build()).execute().use { response ->
                val location = response.header("Location")
                if (response.code !in REDIRECTS || location == null) {
                    return HttpResponse.Answer(url, response.code, response.header("Content-Type"), response.body.string())
                }
                when (val target = origin.resolve(location, url)) {
                    is Target.Inside -> url = target.url
                    is Target.Outside -> return HttpResponse.Left(target.url)
                    Target.Refused -> return HttpResponse.Failed(Failure.Protocol)
                }
                // 307 and 308 repeat the request as it was; the others become a plain fetch.
                if (response.code != 307 && response.code != 308) {
                    method = "GET"
                    body = null
                }
            }
        }
        return HttpResponse.Failed(Failure.Protocol)
    }

    private companion object {
        const val MOST_REDIRECTS = 10
        val REDIRECTS = setOf(301, 302, 303, 307, 308)
    }
}
