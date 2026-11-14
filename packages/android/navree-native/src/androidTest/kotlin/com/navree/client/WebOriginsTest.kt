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
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.util.Collections
import java.util.concurrent.atomic.AtomicBoolean

/**
 * The web view's client on a device: a page of the app's origin that tries to
 * leave it, by a frame, a form posted, a redirect from the server or a
 * script, never makes a request to another origin, and the page stays on the
 * app's origin (or the screen is told it left). The other origin is a second
 * server on this device, on another port.
 */
@RunWith(AndroidJUnit4::class)
class WebOriginsTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val app = MockWebServer()
    private val other = MockWebServer()

    /** Every path the app's server was asked for, and every one the other server was asked for. */
    private val appAsked = Collections.synchronizedList(ArrayList<String>())
    private val otherAsked = Collections.synchronizedList(ArrayList<String>())

    @After
    fun stop() {
        app.close()
        other.close()
    }

    private fun html(body: String) = MockResponse.Builder()
        .code(200)
        .addHeader("Content-Type", "text/html; charset=utf-8")
        .body("<!doctype html><html><head><meta charset=\"utf-8\"></head><body>$body</body></html>")
        .build()

    private fun serve() {
        other.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                otherAsked += request.method + " " + request.url.encodedPath
                return html("<title>Elsewhere</title><p>elsewhere</p>")
            }
        }
        other.start()
        val elsewhere = other.url("/").toString().trimEnd('/')
        app.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                val path = request.url.encodedPath
                appAsked += path
                return when (path) {
                    "/" -> html("<title>Ours</title><p>ours</p>")
                    "/frame" -> html("<p>a frame</p><iframe src=\"$elsewhere/frame\"></iframe>")
                    "/post" -> html("<form id=\"f\" method=\"post\" action=\"$elsewhere/post\"><input name=\"a\" value=\"1\"></form><script>document.getElementById('f').submit()</script>")
                    "/redirect" -> MockResponse.Builder().code(302).addHeader("Location", "$elsewhere/redirected").build()
                    "/script" -> html("<script>location.href = '$elsewhere/script'</script>")
                    else -> MockResponse.Builder().code(404).build()
                }
            }
        }
        app.start()
    }

    @Test
    fun aPageThatTriesToLeaveTheAppsOriginNeverReachesAnother() {
        serve()
        val base = app.url("/").toString().trimEnd('/')
        val origins = Origins(base, loopbackHttp = true)
        val links = Collections.synchronizedList(ArrayList<String>())
        val left = AtomicBoolean(false)
        val finished = Collections.synchronizedList(ArrayList<String>())
        var web: WebView? = null
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
                        web = this
                    }
                },
            )
        }
        compose.waitUntil(10_000) { web != null }

        for (path in listOf("/", "/frame", "/post", "/redirect", "/script")) {
            compose.runOnUiThread { web!!.loadUrl("$base$path") }
            compose.waitUntil(15_000) { path in appAsked }
            // Time for the page to load its frame, post its form or run its script.
            Thread.sleep(3_000)
            compose.waitForIdle()
            var shown: String? = null
            compose.runOnUiThread { shown = web!!.url }
            assertEquals("asked of the other origin after $path", emptyList<String>(), otherAsked.toList())
            assertTrue("after $path the page is $shown", left.get() || shown == null || origins.ours(shown!!))
        }
        assertTrue("the app's own page loaded", finished.any { it.startsWith(base) })
        // Nothing here was tapped; whether the web view counts a redirect after a load as the
        // person's is its own, so what was handed on is written to the log, not asserted.
        android.util.Log.i("WebOriginsTest", "handed to the core: ${links.size}")
    }
}
