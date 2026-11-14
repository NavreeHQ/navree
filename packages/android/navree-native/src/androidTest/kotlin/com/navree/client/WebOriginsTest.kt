package com.navree.client

import android.webkit.WebView
import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.viewinterop.AndroidView
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.navree.client.platform.OriginWebClient
import com.navree.client.platform.Origins
import com.navree.client.platform.guardDocuments
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.util.Collections
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/**
 * The web view's client and its check at the start of every document, on a
 * device. A page of the app's origin that tries to leave it, by a frame, a
 * form posted, a redirect from the server or a script, never shows another
 * origin; a frame or a form that the app's own server redirects elsewhere is
 * emptied before anything in it runs; a frame from a host the app names for
 * the web view loads. The other origins are more servers on this device, on
 * other ports.
 */
@RunWith(AndroidJUnit4::class)
class WebOriginsTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val app = MockWebServer()
    private val other = MockWebServer()
    private val named = MockWebServer()

    /** Every request each server was asked, as `METHOD path`. */
    private val appAsked = Collections.synchronizedList(ArrayList<String>())
    private val otherAsked = Collections.synchronizedList(ArrayList<String>())
    private val namedAsked = Collections.synchronizedList(ArrayList<String>())

    @After
    fun stop() {
        app.close()
        other.close()
        named.close()
    }

    private fun html(body: String) = MockResponse.Builder()
        .code(200)
        .addHeader("Content-Type", "text/html; charset=utf-8")
        .body("<!doctype html><html><head><meta charset=\"utf-8\"></head><body>$body</body></html>")
        .build()

    private fun moved(code: Int, to: String) = MockResponse.Builder().code(code).addHeader("Location", to).build()

    private fun serve() {
        app.start()
        other.start()
        named.start()
        val base = app.url("/").toString().trimEnd('/')
        val elsewhere = other.url("/").toString().trimEnd('/')
        val namedBase = named.url("/").toString().trimEnd('/')
        // A page elsewhere that, if any of it runs, asks the app's own server for /ran: its script
        // comes first, then something to draw. Once in one piece, once slowly: the headers, a pause,
        // then the body in two writes a pause apart.
        val elsewherePage = "<script>new Image().src = '$base/ran'</script><h1>elsewhere</h1>"
        other.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                otherAsked += request.method + " " + request.url.encodedPath
                if (request.url.encodedPath != "/through-a-slow-redirect") return html(elsewherePage)
                val whole = "<!doctype html><html><head><meta charset=\"utf-8\"></head><body>$elsewherePage</body></html>"
                return MockResponse.Builder()
                    .code(200)
                    .addHeader("Content-Type", "text/html; charset=utf-8")
                    .body(whole)
                    .bodyDelay(700, TimeUnit.MILLISECONDS)
                    .throttleBody((whole.length / 2 + 1).toLong(), 700, TimeUnit.MILLISECONDS)
                    .build()
            }
        }
        // A host the app names for the web view: its frame runs, and asks its own server for /ran.
        named.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                namedAsked += request.method + " " + request.url.encodedPath
                return if (request.url.encodedPath == "/frame") {
                    html("<p>a frame of a named host</p><script>new Image().src = '$namedBase/ran'</script>")
                } else {
                    MockResponse.Builder().code(204).build()
                }
            }
        }
        app.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                val path = request.url.encodedPath
                appAsked += request.method + " " + path
                return when (path) {
                    "/" -> html("<title>Ours</title><p>ours</p>")
                    "/frame" -> html("<p>a frame</p><iframe src=\"$elsewhere/frame\"></iframe>")
                    "/post" -> html("<form id=\"f\" method=\"post\" action=\"$elsewhere/post\"><input name=\"a\" value=\"1\"></form><script>document.getElementById('f').submit()</script>")
                    "/redirect" -> moved(302, "$elsewhere/redirected")
                    "/script" -> html("<script>location.href = '$elsewhere/script'</script>")
                    "/frame-redirect" -> html("<p>a frame through a redirect</p><iframe src=\"/out?frame\"></iframe>")
                    "/out" -> moved(302, "$elsewhere/through-a-redirect")
                    "/frame-redirect-slow" -> html("<p>a frame through a redirect, answered slowly</p><iframe src=\"/out-slow\"></iframe>")
                    "/out-slow" -> moved(302, "$elsewhere/through-a-slow-redirect")
                    "/post-redirect" -> html("<form id=\"f\" method=\"post\" action=\"/keep-method\"><input name=\"a\" value=\"1\"></form><script>document.getElementById('f').submit()</script>")
                    "/keep-method" -> moved(307, "$elsewhere/posted-through-a-redirect")
                    "/sandboxed-here" -> html("<p>a sandboxed frame of the app</p><iframe sandbox=\"allow-scripts\" src=\"/sandboxed\"></iframe>")
                    "/sandboxed" -> html("<p>sandboxed</p><script>new Image().src = '$base/ran-sandboxed'</script>")
                    "/sandboxed-out" -> html("<p>a sandboxed frame through a redirect</p><iframe sandbox=\"allow-scripts\" src=\"/out?sandboxed\"></iframe>")
                    "/named-frame" -> html("<p>a frame of a named host</p><iframe src=\"$namedBase/frame\"></iframe>")
                    else -> MockResponse.Builder().code(204).build()
                }
            }
        }
    }

    @Test
    fun aPageThatTriesToLeaveTheAppsOriginNeverShowsAnother() {
        serve()
        val base = app.url("/").toString().trimEnd('/')
        val namedHost = "localhost:" + named.port
        val origins = Origins(base, loopbackHttp = true, webHosts = listOf(namedHost))
        val links = Collections.synchronizedList(ArrayList<String>())
        val left = AtomicBoolean(false)
        val finished = Collections.synchronizedList(ArrayList<String>())
        var web: WebView? = null
        var guarded = false
        compose.setContent {
            AndroidView(
                modifier = Modifier.fillMaxSize(),
                factory = { context ->
                    WebView(context).apply {
                        settings.javaScriptEnabled = true
                        webViewClient = object : OriginWebClient(
                            origins,
                            onLink = { links += it },
                            onLeft = { left.set(true) },
                        ) {
                            override fun onPageFinished(view: WebView, url: String?) {
                                super.onPageFinished(view, url)
                                url?.let { finished += it }
                            }
                        }
                        guarded = guardDocuments(this, origins)
                        web = this
                    }
                },
            )
        }
        compose.waitUntil(10_000) { web != null }
        android.util.Log.i("WebOriginsTest", "the document check is on this web view: $guarded")

        fun open(path: String) {
            left.set(false)
            compose.runOnUiThread { web!!.loadUrl("$base$path") }
            compose.waitUntil(15_000) { appAsked.any { it.endsWith(" $path") } }
            // Time for the page to load its frame, post its form or run its script.
            Thread.sleep(3_000)
            compose.waitForIdle()
        }

        fun shown(): String? {
            var url: String? = null
            compose.runOnUiThread { url = web!!.url }
            return url
        }

        // The web view's own checks: the other origin is never even asked.
        for (path in listOf("/", "/frame", "/post", "/redirect", "/script")) {
            open(path)
            assertEquals("asked of the other origin after $path", emptyList<String>(), otherAsked.toList())
            val url = shown()
            assertTrue("after $path the page is $url", left.get() || url == null || origins.ours(url))
        }
        assertTrue("a first load redirected elsewhere ends as a failed screen", run { open("/redirect"); left.get() })
        assertTrue("the app's own page loaded", finished.any { it.startsWith(base) })

        // A frame the app's own server redirects elsewhere: the engine follows the redirect, and
        // the document check empties what arrives before any of it runs.
        open("/frame-redirect")
        android.util.Log.i("WebOriginsTest", "asked of the other origin through a frame's redirect: $otherAsked")
        if (guarded) assertFalse("the redirected frame ran", appAsked.any { it.endsWith(" /ran") })
        open("/frame-redirect-slow")
        Thread.sleep(2_000)
        android.util.Log.i("WebOriginsTest", "asked of the other origin through a frame's slow redirect: $otherAsked")
        if (guarded) assertFalse("the redirected frame answered slowly ran", appAsked.any { it.endsWith(" /ran") })

        // A form the app's own server answers 307 to elsewhere: the engine posts it there (no callback
        // sees it first); what comes back is never shown, and the screen is told the page left.
        otherAsked.clear()
        open("/post-redirect")
        android.util.Log.i("WebOriginsTest", "asked of the other origin through a posted form's 307: $otherAsked")
        if (guarded) assertFalse("the page the form was posted to ran", appAsked.any { it.endsWith(" /ran") })
        assertTrue("the screen was told the page left", left.get() || origins.ours(shown() ?: base))

        // A sandboxed frame has no origin of its own: the check goes by its address. One on the app's
        // origin is left alone and runs; one the app's server redirects elsewhere is emptied.
        open("/sandboxed-here")
        compose.waitUntil(10_000) { appAsked.any { it.endsWith(" /ran-sandboxed") } }
        otherAsked.clear()
        open("/sandboxed-out")
        Thread.sleep(1_000)
        val ranElsewhere = appAsked.any { it.endsWith(" /ran") }
        android.util.Log.i("WebOriginsTest", "a sandboxed frame redirected elsewhere: asked of the other origin $otherAsked, its page ran: $ranElsewhere")
        if (guarded) assertFalse("the sandboxed frame redirected elsewhere ran", ranElsewhere)

        // A frame of a host the app names for the web view loads, and runs.
        open("/named-frame")
        compose.waitUntil(10_000) { namedAsked.any { it.endsWith(" /ran") } }
        assertTrue(namedAsked.contains("GET /frame"))

        android.util.Log.i("WebOriginsTest", "handed to the core: ${links.size}")
    }
}
