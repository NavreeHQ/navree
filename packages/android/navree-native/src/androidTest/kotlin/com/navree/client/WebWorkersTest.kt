package com.navree.client

import android.webkit.ServiceWorkerClient
import android.webkit.ServiceWorkerController
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.viewinterop.AndroidView
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.webkit.WebViewFeature
import com.navree.client.platform.OriginWebClient
import com.navree.client.platform.Origins
import com.navree.client.platform.WorkerGuard
import com.navree.client.platform.guardDocuments
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.CopyOnWriteArrayList

/**
 * What a page of the app starts in the web view beside its own loads: a
 * service worker it registers, a worker of its own, and a WebSocket it opens.
 * The other origin is another server on this device. Each rule is shown with
 * a control that has no rule, so the test can fail: without the rule the other
 * server is reached. A service worker's own requests go through the web
 * rules as a page's do. A socket and a worker are not requests the web view's
 * client is asked about: the page's own scripts are held by the check at the
 * start of every document, and what a worker that is already running does is
 * said by the tests that name it.
 */
@RunWith(AndroidJUnit4::class)
class WebWorkersTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val app = MockWebServer()
    private val other = MockWebServer()

    /** What the app's server was asked, as `METHOD path`. */
    private val appAsked = CopyOnWriteArrayList<String>()

    /** What the other server was asked, as `METHOD path` (a socket's upgrade as `UPGRADE path`). */
    private val otherAsked = CopyOnWriteArrayList<String>()

    /** What the page told the app's server (`/says?m=...`), in order. */
    private val said = CopyOnWriteArrayList<String>()

    /** Every address the web view's client was asked about. */
    private val seen = CopyOnWriteArrayList<String>()

    /** Sockets the app's server accepted. */
    private val ownSockets = CopyOnWriteArrayList<String>()

    private lateinit var base: String
    private lateinit var elsewhere: String

    @After
    fun stop() {
        app.close()
        other.close()
    }

    /** The `Content-Security-Policy` the app's server sends with its page and with its worker scripts, when a test names one. */
    private var pagePolicy: String? = null
    private var scriptPolicy: String? = null

    private fun page(body: String) = MockResponse.Builder()
        .code(200)
        .addHeader("Content-Type", "text/html; charset=utf-8")
        .apply { pagePolicy?.let { addHeader("Content-Security-Policy", it) } }
        .body("<!doctype html><html><head><meta charset=\"utf-8\"></head><body>$body</body></html>")
        .build()

    private fun script(body: String) = MockResponse.Builder()
        .code(200)
        .addHeader("Content-Type", "text/javascript")
        .apply { scriptPolicy?.let { addHeader("Content-Security-Policy", it) } }
        .body(body)
        .build()

    /**
     * The two servers. The app's answers `/` with [home] and a path of [files] with its script;
     * both are read when asked, so they may name the other server's address and port.
     */
    private fun serve(home: () -> String, files: () -> Map<String, String> = { emptyMap() }) {
        app.start()
        other.start()
        base = app.url("/").toString().trimEnd('/')
        elsewhere = other.url("/").toString().trimEnd('/')
        other.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                val path = request.url.encodedPath
                if (request.headers["Upgrade"].equals("websocket", ignoreCase = true)) {
                    otherAsked += "UPGRADE $path"
                    return MockResponse.Builder().webSocketUpgrade(object : WebSocketListener() {}).build()
                }
                otherAsked += request.method + " " + path
                if (path.endsWith(".js")) return script("1;")
                return MockResponse.Builder().code(200).addHeader("Access-Control-Allow-Origin", "*").body("elsewhere").build()
            }
        }
        app.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                val path = request.url.encodedPath
                appAsked += request.method + " " + path
                return when {
                    path == "/says" -> {
                        said += request.url.queryParameter("m") ?: ""
                        MockResponse.Builder().code(204).build()
                    }
                    path == "/own-socket" -> MockResponse.Builder().webSocketUpgrade(object : WebSocketListener() {
                        override fun onOpen(webSocket: WebSocket, response: Response) {
                            ownSockets += "opened"
                        }
                    }).build()
                    path == "/" -> page(home())
                    files().containsKey(path) -> script(files().getValue(path))
                    else -> MockResponse.Builder().code(204).build()
                }
            }
        }
    }

    /**
     * A web view with the client's checks, loading the app's page. With [documents] false the
     * check at the start of every document is not added; with [rules] false the web view's client
     * is the system's own and checks nothing: the controls.
     */
    private fun openedBy(origins: Origins, documents: Boolean = true, rules: Boolean = true) {
        var web: WebView? = null
        var added = !documents
        compose.setContent {
            AndroidView(
                modifier = Modifier.fillMaxSize(),
                factory = { context ->
                    WebView(context).apply {
                        settings.javaScriptEnabled = true
                        webViewClient = if (rules) {
                            object : OriginWebClient(origins, onLink = {}) {
                                override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? {
                                    seen += request.url.toString()
                                    return super.shouldInterceptRequest(view, request)
                                }
                            }
                        } else {
                            object : WebViewClient() {
                                override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? {
                                    seen += request.url.toString()
                                    return null
                                }
                            }
                        }
                        if (documents) added = guardDocuments(this, origins)
                        web = this
                    }
                },
            )
        }
        compose.waitUntil(10_000) { web != null }
        assertTrue("the check at the start of every document was added", added)
        compose.runOnUiThread { web!!.loadUrl("$base/") }
        compose.waitUntil(15_000) { appAsked.any { it.endsWith(" /") } }
    }

    /** Waits until the page has said [count] things. */
    private fun waitForSaid(count: Int = 1, seconds: Int = 30) {
        assertTrue("the page said $count things in $seconds s: $said", waitFor(seconds) { said.size >= count })
    }

    private fun waitFor(seconds: Int, what: () -> Boolean): Boolean {
        val end = System.currentTimeMillis() + seconds * 1_000L
        while (System.currentTimeMillis() < end) {
            if (what()) return true
            Thread.sleep(250)
        }
        return what()
    }

    /** Time for what a page started to reach, if it can reach anything, then what everyone was asked is written beside the pictures. */
    private fun settle(file: String, what: String) {
        Thread.sleep(3_000)
        compose.waitForIdle()
        val all = "$what\nthe page said: $said\nthe app was asked: $appAsked\nthe other origin was asked: $otherAsked\n" +
            "the client was asked about: ${seen.filter { it.startsWith(elsewhere) }}\nthe app's own sockets: $ownSockets\n"
        android.util.Log.i("WebWorkersTest", "$file: $all")
        Shots.put(file, all)
    }

    private fun documentChecks() = assumeTrue(
        "this system's web view has no script at the start of a document",
        WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT),
    )

    // ------------------------------------------------------------------ service workers

    private fun serviceWorker() =
        "self.addEventListener('install', function(e){ self.skipWaiting(); e.waitUntil(fetch('$elsewhere/from-install', {mode: 'no-cors'}).catch(function(){})) });" +
            "self.addEventListener('activate', function(e){ e.waitUntil(self.clients.claim().then(function(){ return fetch('$elsewhere/from-activate', {mode: 'no-cors'}).catch(function(){}) })) });" +
            "self.addEventListener('fetch', function(e){ if (new URL(e.request.url).pathname === '/through-the-worker') e.respondWith(fetch('$elsewhere/from-fetch', {mode: 'cors'}).then(function(r){ return r.text() }).then(function(t){ return new Response('reached:' + t) }, function(){ return new Response('blocked') })) });"

    // The page registers the worker, waits until it controls the page, and says what a fetch the worker answers came to.
    private val registers = "<p>ours</p><script>navigator.serviceWorker.register('/sw.js').then(function(){ return navigator.serviceWorker.ready }).then(function(){ return navigator.serviceWorker.controller ? 0 : new Promise(function(done){ navigator.serviceWorker.addEventListener('controllerchange', done) }) }).then(function(){ return fetch('/through-the-worker') }).then(function(r){ return r.text() })" +
        ".then(function(t){ new Image().src = '/says?m=' + encodeURIComponent(t) }, function(e){ new Image().src = '/says?m=' + encodeURIComponent('failed: ' + e) })</script>"

    @Test
    fun aServiceWorkerReachesAnotherOriginWhereNothingGuardsIt() {
        serve({ registers }, { mapOf("/sw.js" to serviceWorker()) })
        // The control: a client for service workers that lets every request through.
        compose.runOnUiThread {
            ServiceWorkerController.getInstance().setServiceWorkerClient(object : ServiceWorkerClient() {
                override fun shouldInterceptRequest(request: WebResourceRequest): WebResourceResponse? = null
            })
        }
        openedBy(Origins(base, loopbackHttp = true))
        waitForSaid()
        settle("worker-control.txt", "no rules on service workers")
        assertEquals("the worker ran its fetch handler and its request reached the other origin", listOf("reached:elsewhere"), said.toList())
        assertTrue("the other origin was asked by the worker: $otherAsked", otherAsked.contains("GET /from-fetch"))
    }

    @Test
    fun aServiceWorkerNeverReachesAnotherOrigin() {
        serve({ registers }, { mapOf("/sw.js" to serviceWorker()) })
        val origins = Origins(base, loopbackHttp = true)
        // As the app does: armed at start, made before the first web view.
        var inForce = false
        compose.runOnUiThread {
            WorkerGuard.arm(origins)
            inForce = WorkerGuard.ensure()
        }
        assertTrue("the rules for service workers are in force", inForce)
        openedBy(origins)
        waitForSaid()
        settle("worker-asked.txt", "rules on service workers in force: $inForce")
        assertEquals("the worker ran its fetch handler, and its request was refused", listOf("blocked"), said.toList())
        assertEquals("a worker reached the other origin: $otherAsked", emptyList<String>(), otherAsked.toList())
    }

    // ------------------------------------------------------------------ a worker the page starts from one of the app's addresses

    private fun reportsWhatItReaches() =
        "fetch('$elsewhere/from-worker', {mode: 'cors'}).then(function(r){ return r.text() }).then(function(t){ postMessage('fetch reached:' + t) }, function(){ postMessage('fetch blocked') });" +
            "try { importScripts('$elsewhere/import.js'); postMessage('import reached') } catch (e) { postMessage('import blocked') }"

    private fun pageWithAWorker() =
        "<p>ours</p><script>var w = new Worker('/w.js'); w.onmessage = function(e){ new Image().src = '/says?m=' + encodeURIComponent(e.data) }; w.onerror = function(){ new Image().src = '/says?m=worker-error' }</script>"

    @Test
    fun aWorkerOfThePagesOwnNeverReachesAnotherOrigin() {
        // A dedicated worker's fetch and importScripts are asked of the web view's client, which refuses them.
        serve({ pageWithAWorker() }, { mapOf("/w.js" to reportsWhatItReaches()) })
        openedBy(Origins(base, loopbackHttp = true))
        waitForSaid(2)
        settle("dedicated-worker.txt", "a worker of the page's own, started from /w.js, with the client's rules")
        assertEquals("what the worker was told", listOf("fetch blocked", "import blocked"), said.sorted())
        assertEquals("the other origin was asked: $otherAsked", emptyList<String>(), otherAsked.toList())
        assertTrue("the worker's requests were asked of the client: $seen", seen.contains("$elsewhere/from-worker") && seen.contains("$elsewhere/import.js"))
    }

    @Test
    fun aWorkerOfThePagesOwnReachesAnotherOriginWhereTheClientHasNoRules() {
        serve({ pageWithAWorker() }, { mapOf("/w.js" to reportsWhatItReaches()) })
        openedBy(Origins(base, loopbackHttp = true), documents = false, rules = false)
        waitForSaid(2)
        settle("dedicated-worker-control.txt", "a worker of the page's own, with a client that checks nothing")
        assertEquals("what the worker was told", listOf("fetch reached:elsewhere", "import reached"), said.sorted())
        assertTrue("the other origin was asked by the worker: $otherAsked", otherAsked.contains("GET /from-worker"))
    }

    // ------------------------------------------------------------------ workers made from an address of their own

    private fun socketOfTheWorker() = "new WebSocket('ws://localhost:${other.port}/socket')"

    private fun madeOrRefused(name: String, body: String) =
        "try { $body; new Image().src = '/says?m=$name-made' } catch (e) { new Image().src = '/says?m=$name-refused' };"

    private fun workersOfTheirOwnAddress() =
        "<p>ours</p><script>" +
            madeOrRefused("blob", "new Worker(URL.createObjectURL(new Blob([\"${socketOfTheWorker()}\"], {type: 'text/javascript'})))") +
            madeOrRefused("data", "new Worker('data:text/javascript,' + encodeURIComponent(\"${socketOfTheWorker()}\"))") +
            madeOrRefused("own", "new Worker('/own.js')") +
            madeOrRefused("constructor", "new (new Worker('/own.js').constructor)(URL.createObjectURL(new Blob([\"${socketOfTheWorker()}\"], {type: 'text/javascript'})))") +
            // An address that answers one thing the first time it is turned into a string and a blob address the second.
            madeOrRefused("toString", "var n = 0, b = URL.createObjectURL(new Blob([\"${socketOfTheWorker()}\"], {type: 'text/javascript'})); new Worker({toString: function(){ return n++ ? b : '/own.js' }})") +
            "</script>"

    @Test
    fun aWorkerMadeFromABlobOrDataAddressIsRefused() {
        documentChecks()
        serve({ workersOfTheirOwnAddress() }, { mapOf("/own.js" to "1;") })
        openedBy(Origins(base, loopbackHttp = true))
        waitForSaid(5)
        settle("workers-from-addresses.txt", "with the check at the start of every document")
        assertEquals(
            "a blob, a data and one made through another worker's constructor are refused; one of the app's own is made, and so is the one whose address is turned into a string once",
            listOf("blob-refused", "data-refused", "own-made", "constructor-refused", "toString-made").sorted(),
            said.sorted(),
        )
        assertEquals("nothing reached the other origin: $otherAsked", emptyList<String>(), otherAsked.toList())
    }

    @Test
    fun aWorkerMadeFromABlobAddressRunsAndReachesAnotherOriginWhereNothingChecksTheDocument() {
        serve({ workersOfTheirOwnAddress() }, { mapOf("/own.js" to "1;") })
        openedBy(Origins(base, loopbackHttp = true), documents = false)
        waitForSaid(5)
        assertTrue("the worker's socket reached the other origin: $otherAsked", waitFor(15) { otherAsked.any { it.startsWith("UPGRADE") } })
        settle("workers-from-addresses-control.txt", "without the check at the start of every document")
        assertTrue("a blob worker was made: $said", said.contains("blob-made"))
    }

    // ------------------------------------------------------------------ sockets

    private fun socketsOfThePage() =
        "<p>ours</p><script>" +
            madeOrRefused("direct", "new WebSocket('ws://localhost:${other.port}/socket')") +
            madeOrRefused("constructor", "var s = new WebSocket('ws://localhost:${app.port}/own-socket'); new s.constructor('ws://localhost:${other.port}/socket')") +
            madeOrRefused("own", "var t = new WebSocket('ws://localhost:${app.port}/own-socket')") +
            // An object that answers the app's own socket the first time it is turned into a string and the other server the second.
            madeOrRefused("toString", "var n = 0; new WebSocket({toString: function(){ return n++ ? 'ws://localhost:${other.port}/socket' : 'ws://localhost:${app.port}/own-socket' }})") +
            "</script>"

    @Test
    fun aSocketGoesOnlyToTheAppsOrigins() {
        documentChecks()
        serve({ socketsOfThePage() })
        openedBy(Origins(base, loopbackHttp = true))
        waitForSaid(4)
        settle("socket-asked.txt", "with the check at the start of every document")
        assertEquals("what the page was told", listOf("direct-refused", "constructor-refused", "own-made", "toString-made").sorted(), said.sorted())
        assertEquals("a socket reached another origin: $otherAsked", emptyList<String>(), otherAsked.toList())
        assertTrue("the page's own socket opens: $ownSockets", waitFor(5) { ownSockets.isNotEmpty() })
    }

    @Test
    fun aSocketReachesAnotherOriginWhereNothingChecksTheDocument() {
        serve({ socketsOfThePage() })
        openedBy(Origins(base, loopbackHttp = true), documents = false)
        waitForSaid(4)
        assertTrue("the socket reached the other origin: $otherAsked", waitFor(10) { otherAsked.any { it.startsWith("UPGRADE") } })
        settle("socket-control.txt", "without the check at the start of every document")
    }

    // What the client cannot cover: the first, empty document of a frame the page makes and has not navigated
    // does not get the script at the start of a document, so a socket made through it is not checked. A frame
    // given a document (srcdoc, or an address) is.
    @Test
    fun aSocketOpenedFromAnEmptyFrameThePageMakesIsNotChecked() {
        documentChecks()
        serve({
            "<p>ours</p><script>" +
                madeOrRefused("empty-frame", "var f = document.createElement('iframe'); document.body.appendChild(f); new f.contentWindow.WebSocket('ws://localhost:${other.port}/socket')") +
                "</script><iframe srcdoc=\"<script>try { new WebSocket('ws://localhost:${other.port}/socket'); new Image().src = '/says?m=srcdoc-made' } catch (e) { new Image().src = '/says?m=srcdoc-refused' }</script>\"></iframe>"
        })
        openedBy(Origins(base, loopbackHttp = true))
        waitForSaid(2)
        settle("socket-in-frame.txt", "a socket opened from a frame the page made, an empty one and one with srcdoc")
        assertEquals("what the frames were told", listOf("empty-frame-made", "srcdoc-refused"), said.sorted())
        assertEquals("only the empty frame's socket reached the other origin: $otherAsked", listOf("UPGRADE /socket"), otherAsked.toList())
    }

    // What the client cannot cover: the check at the start of a document does not run inside a worker, and a
    // socket is not a request the client is asked about. This test says so; the day it fails, the README
    // says something that is no longer true.
    @Test
    fun aSocketOpenedInsideAWorkerIsNotCheckedByTheClient() {
        documentChecks()
        serve(
            { "<p>ours</p><script>var w = new Worker('/socket-worker.js'); w.onmessage = function(e){ new Image().src = '/says?m=' + encodeURIComponent(e.data) }</script>" },
            { mapOf("/socket-worker.js" to "var s = new WebSocket('ws://localhost:${other.port}/socket'); s.onopen = function(){ postMessage('socket opened') }; s.onerror = function(){ postMessage('socket failed') }") },
        )
        openedBy(Origins(base, loopbackHttp = true))
        waitForSaid()
        settle("socket-in-worker.txt", "a socket opened inside a worker the page started from one of the app's addresses")
        assertEquals("the worker's socket opened", listOf("socket opened"), said.toList())
        assertTrue("the socket reached the other origin: $otherAsked", otherAsked.any { it.startsWith("UPGRADE") })
        assertEquals("the client was not asked about the socket: $seen", emptyList<String>(), seen.filter { it.startsWith(elsewhere) })
    }

    // The page's own policy and a worker loaded from an address: per MDN ("Using web workers", the section on
    // content security policy) a worker is governed by the policy sent with its own script, and only a worker
    // from a blob: or data: address inherits the page's. So a page that says connect-src 'self' does not close a
    // socket inside a worker whose script came without a header. The system's own client is used: nothing of the
    // client's is in play, so the policies are all that is tested.
    private fun pageWithASocketWorker() =
        "<p>ours</p><script>var w = new Worker('/socket-worker.js'); w.onmessage = function(e){ new Image().src = '/says?m=' + encodeURIComponent(e.data) }</script>"

    private fun socketWorker() =
        "var s = new WebSocket('ws://localhost:${other.port}/socket'); s.onopen = function(){ postMessage('socket opened') }; s.onerror = function(){ postMessage('socket failed') }; s.onclose = function(){ postMessage('socket failed') }"

    @Test
    fun aPagePolicyDoesNotHoldASocketInsideAWorkerLoadedFromAnAddress() {
        pagePolicy = "connect-src 'self'"
        serve({ pageWithASocketWorker() }, { mapOf("/socket-worker.js" to socketWorker()) })
        openedBy(Origins(base, loopbackHttp = true), documents = false, rules = false)
        waitForSaid()
        settle("socket-in-worker-page-policy.txt", "connect-src 'self' on the page, none on the worker's script")
        assertEquals("the worker's socket opened in spite of the page's policy", listOf("socket opened"), said.toList())
        assertTrue("the socket reached the other origin: $otherAsked", otherAsked.any { it.startsWith("UPGRADE") })
    }

    @Test
    fun aPolicySentWithTheWorkersScriptHoldsItsSocket() {
        pagePolicy = "connect-src 'self'"
        scriptPolicy = "connect-src 'self'"
        serve({ pageWithASocketWorker() }, { mapOf("/socket-worker.js" to socketWorker()) })
        openedBy(Origins(base, loopbackHttp = true), documents = false, rules = false)
        waitForSaid()
        settle("socket-in-worker-script-policy.txt", "connect-src 'self' on the worker's script")
        assertTrue("the socket was stopped: $said", said.isNotEmpty() && said.all { it == "socket failed" })
        assertEquals("nothing reached the other origin: $otherAsked", emptyList<String>(), otherAsked.toList())
    }

    @Test
    fun aWorkerSrcPolicyOnThePageStopsWorkersFromBlobAndDataAddresses() {
        pagePolicy = "worker-src 'self'"
        serve({ workersOfTheirOwnAddress() }, { mapOf("/own.js" to "1;") })
        openedBy(Origins(base, loopbackHttp = true), documents = false, rules = false)
        waitForSaid(5)
        settle("workers-from-addresses-worker-src.txt", "worker-src 'self' on the page, with the system's own client and no document script")
        assertEquals("a blob and a data worker did not run: $otherAsked", emptyList<String>(), otherAsked.toList())
    }
}
