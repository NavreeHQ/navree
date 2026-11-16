package com.navree.client

import android.webkit.WebView
import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.viewinterop.AndroidView
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.webkit.WebViewFeature
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
import org.junit.Assume.assumeTrue
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
    private val appAsked = java.util.concurrent.CopyOnWriteArrayList<String>()
    private val otherAsked = java.util.concurrent.CopyOnWriteArrayList<String>()
    private val namedAsked = java.util.concurrent.CopyOnWriteArrayList<String>()

    @After
    fun stop() {
        app.close()
        other.close()
        named.close()
    }

    private fun html(body: String, policy: String? = null) = MockResponse.Builder()
        .code(200)
        .addHeader("Content-Type", "text/html; charset=utf-8")
        .apply { policy?.let { addHeader("Content-Security-Policy", it) } }
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
                    // A picture, a script and a fetch of the app's own addresses that the app's server answers with a
                    // redirect to another origin; once without a policy, once with one that names only the app.
                    "/redirected-loads" -> html(redirectedLoads)
                    "/redirected-loads-with-policy" -> html(redirectedLoads, policy = "default-src 'self'; script-src 'self' 'unsafe-inline'")
                    "/img-out" -> moved(302, "$elsewhere/img")
                    "/script-out" -> moved(302, "$elsewhere/script.js")
                    "/fetch-out" -> moved(302, "$elsewhere/fetch")
                    else -> MockResponse.Builder().code(204).build()
                }
            }
        }
    }

    private val redirectedLoads = "<p>loads through a redirect</p><img src=\"/img-out\"><script src=\"/script-out\"></script>" +
        "<script>fetch('/fetch-out').catch(function(){})</script>"

    /** Every address the web view's client was asked about. */
    private val asked = java.util.concurrent.CopyOnWriteArrayList<String>()

    private val links = java.util.concurrent.CopyOnWriteArrayList<String>()
    private val pageLeft = AtomicBoolean(false)
    private val finished = java.util.concurrent.CopyOnWriteArrayList<String>()
    private var web: WebView? = null
    private lateinit var base: String
    private lateinit var origins: Origins

    /**
     * A web view with the client's checks, with or without the check at the start of every
     * document (the control); says whether that check was added.
     */
    private fun webView(documents: Boolean, namedHost: Boolean = false): Boolean {
        serve()
        base = app.url("/").toString().trimEnd('/')
        origins = Origins(base, loopbackHttp = true, webHosts = if (namedHost) listOf("localhost:" + named.port) else emptyList())
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
                            onLeft = { pageLeft.set(true) },
                        ) {
                            override fun shouldInterceptRequest(view: WebView, request: android.webkit.WebResourceRequest): android.webkit.WebResourceResponse? {
                                asked += request.url.toString()
                                return super.shouldInterceptRequest(view, request)
                            }

                            override fun onPageFinished(view: WebView, url: String?) {
                                super.onPageFinished(view, url)
                                url?.let { finished += it }
                            }
                        }
                        if (documents) guarded = guardDocuments(this, origins)
                        web = this
                    }
                },
            )
        }
        compose.waitUntil(10_000) { web != null }
        return guarded
    }

    private fun open(path: String) {
        pageLeft.set(false)
        compose.runOnUiThread { web!!.loadUrl("$base$path") }
        compose.waitUntil(15_000) { appAsked.any { it.endsWith(" $path") } }
        // Time for the page to load its frame, post its form or run its script.
        Thread.sleep(3_000)
        compose.waitForIdle()
    }

    private fun shown(): String? {
        var url: String? = null
        compose.runOnUiThread { url = web!!.url }
        return url
    }

    private fun documentChecks() = assumeTrue(
        "this system's web view has no script at the start of a document",
        WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT),
    )

    @Test
    fun aPageThatTriesToLeaveTheAppsOriginNeverShowsAnother() {
        webView(documents = true, namedHost = true)
        // The web view's own checks: the other origin is never even asked.
        for (path in listOf("/", "/frame", "/post", "/redirect", "/script")) {
            open(path)
            assertEquals("asked of the other origin after $path", emptyList<String>(), otherAsked.toList())
            val url = shown()
            assertTrue("after $path the page is $url", pageLeft.get() || url == null || origins.ours(url))
        }
        assertTrue("a first load redirected elsewhere ends as a failed screen", run { open("/redirect"); pageLeft.get() })
        assertTrue("the app's own page loaded", finished.any { it.startsWith(base) })

        // A frame of a host the app names for the web view loads, and runs.
        open("/named-frame")
        compose.waitUntil(10_000) { namedAsked.any { it.endsWith(" /ran") } }
        assertTrue(namedAsked.contains("GET /frame"))
        android.util.Log.i("WebOriginsTest", "handed to the core: ${links.size}")
    }

    // A frame or a form the app's own server sends elsewhere by a redirect is not seen by the client's callbacks:
    // the engine follows it, and only the check at the start of every document stops what arrives.

    @Test
    fun aFrameOrAFormRedirectedElsewhereIsEmptiedBeforeAnythingInItRuns() {
        documentChecks()
        assertTrue("the check at the start of every document was added", webView(documents = true))
        open("/frame-redirect")
        android.util.Log.i("WebOriginsTest", "asked of the other origin through a frame's redirect: $otherAsked")
        assertFalse("the redirected frame ran", appAsked.any { it.endsWith(" /ran") })
        open("/frame-redirect-slow")
        Thread.sleep(2_000)
        android.util.Log.i("WebOriginsTest", "asked of the other origin through a frame's slow redirect: $otherAsked")
        assertFalse("the redirected frame answered slowly ran", appAsked.any { it.endsWith(" /ran") })

        // A form the app's own server answers 307 to elsewhere: the engine posts it there; what comes back
        // is never shown, and the screen is told the page left.
        otherAsked.clear()
        open("/post-redirect")
        android.util.Log.i("WebOriginsTest", "asked of the other origin through a posted form's 307: $otherAsked")
        assertFalse("the page the form was posted to ran", appAsked.any { it.endsWith(" /ran") })
        assertTrue("the screen was told the page left", pageLeft.get() || origins.ours(shown() ?: base))

        // A sandboxed frame has no origin of its own: the check goes by its address. One on the app's
        // origin is left alone and runs; one the app's server redirects elsewhere is emptied.
        open("/sandboxed-here")
        compose.waitUntil(10_000) { appAsked.any { it.endsWith(" /ran-sandboxed") } }
        otherAsked.clear()
        open("/sandboxed-out")
        Thread.sleep(1_000)
        android.util.Log.i("WebOriginsTest", "a sandboxed frame redirected elsewhere: asked of the other origin $otherAsked")
        assertFalse("the sandboxed frame redirected elsewhere ran", appAsked.any { it.endsWith(" /ran") })
    }

    // What the client cannot check: the platform asks the web view's client about the first address of a request,
    // and "in the case of redirects, this is only called for the initial resource URL, not any subsequent redirect
    // URLs" (WebViewClient.shouldInterceptRequest). A picture, a script and a fetch of the app's own address that the
    // app's server redirects elsewhere are loaded from there; the README says so, and the day this test fails it
    // says something that is no longer true. What holds them is the page's own policy, which the engine checks on
    // every hop: the test after this one.
    @Test
    fun aPictureAScriptAndAFetchRedirectedElsewhereAreLoadedFromThereAndTheClientIsAskedOnlyTheFirstAddress() {
        webView(documents = true)
        open("/redirected-loads")
        val reached = listOf("GET /img", "GET /script.js", "GET /fetch")
        runCatching { compose.waitUntil(15_000) { otherAsked.containsAll(reached) } }
        val elsewhere = other.url("/").toString().trimEnd('/')
        val text = "the app was asked: $appAsked\nthe other origin was asked: $otherAsked\nthe client was asked about: $asked\n"
        android.util.Log.i("WebOriginsTest", "redirected loads: $text")
        Shots.put("redirect-loads.txt", "a picture, a script and a fetch redirected to another origin, no policy\n$text")
        assertTrue("the other origin was asked for each: $otherAsked", otherAsked.containsAll(reached))
        assertTrue("the client was asked about each first address: $asked", listOf("/img-out", "/script-out", "/fetch-out").all { path -> asked.any { it.endsWith(path) } })
        assertEquals("the client was never asked about an address on the other origin: $asked", emptyList<String>(), asked.filter { it.startsWith(elsewhere) })
    }

    @Test
    fun aPictureAScriptAndAFetchRedirectedElsewhereAreStoppedByThePagesOwnPolicy() {
        webView(documents = true)
        open("/redirected-loads-with-policy")
        compose.waitUntil(15_000) { listOf("/img-out", "/script-out", "/fetch-out").all { path -> appAsked.contains("GET $path") } }
        // As long as the test before waits for the other origin to be asked.
        Thread.sleep(15_000)
        Shots.put("redirect-loads-policy.txt", "the same with default-src 'self' on the page\nthe app was asked: $appAsked\nthe other origin was asked: $otherAsked\n")
        assertEquals("the other origin was asked: $otherAsked", emptyList<String>(), otherAsked.toList())
    }

    @Test
    fun aFrameRedirectedElsewhereRunsWhereNothingChecksTheDocument() {
        webView(documents = false)
        open("/frame-redirect")
        assertTrue("the page of the other origin, in the frame, ran: $appAsked", appAsked.any { it.endsWith(" /ran") })
        assertTrue("the other origin was asked for it: $otherAsked", otherAsked.contains("GET /through-a-redirect"))
    }
}
