package com.navree.client

import android.webkit.ServiceWorkerClient
import android.webkit.ServiceWorkerController
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
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

    private fun page(body: String) = MockResponse.Builder()
        .code(200)
        .addHeader("Content-Type", "text/html; charset=utf-8")
        .body("<!doctype html><html><head><meta charset=\"utf-8\"></head><body>$body</body></html>")
        .build()

    private fun script(body: String) = MockResponse.Builder().code(200).addHeader("Content-Type", "text/javascript").body(body).build()

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
     * check at the start of every document is not added: the control.
     */
    private fun openedBy(origins: Origins, documents: Boolean = true) {
        var web: WebView? = null
        var added = !documents
        compose.setContent {
            AndroidView(
                modifier = Modifier.fillMaxSize(),
                factory = { context ->
                    WebView(context).apply {
                        settings.javaScriptEnabled = true
                        webViewClient = object : OriginWebClient(origins, onLink = {}) {
                            override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? {
                                seen += request.url.toString()
                                return super.shouldInterceptRequest(view, request)
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
    private val registers = "<p>ours</p><script>navigator.serviceWorker.register('/sw.js').then(function(){ return navigator.serviceWorker.ready }).then(function(){ return fetch('/through-the-worker') }).then(function(r){ return r.text() })" +
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

    @Test
    fun whatAWorkerOfThePagesOwnReachesIsRecordedWithWhatTheClientWasAskedAbout() {
        serve(
            { "<p>ours</p><script>var w = new Worker('/w.js'); w.onmessage = function(e){ new Image().src = '/says?m=' + encodeURIComponent(e.data) }; w.onerror = function(){ new Image().src = '/says?m=worker-error' }</script>" },
            { mapOf("/w.js" to reportsWhatItReaches()) },
        )
        openedBy(Origins(base, loopbackHttp = true))
        waitForSaid(2)
        settle("dedicated-worker.txt", "a worker of the page's own, started from /w.js, with the client's rules on documents")
        assertTrue("the worker ran: $said", said.isNotEmpty())
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
            "</script>"

    @Test
    fun aWorkerMadeFromABlobOrDataAddressIsRefused() {
        documentChecks()
        serve({ workersOfTheirOwnAddress() }, { mapOf("/own.js" to "1;") })
        openedBy(Origins(base, loopbackHttp = true))
        waitForSaid(4)
        settle("workers-from-addresses.txt", "with the check at the start of every document")
        assertEquals(
            "a blob, a data and one made through another worker's constructor are refused; one of the app's own is made",
            listOf("blob-refused", "data-refused", "own-made", "constructor-refused").sorted(),
            said.sorted(),
        )
        assertEquals("nothing reached the other origin: $otherAsked", emptyList<String>(), otherAsked.toList())
    }

    @Test
    fun aWorkerMadeFromABlobAddressRunsAndReachesAnotherOriginWhereNothingChecksTheDocument() {
        serve({ workersOfTheirOwnAddress() }, { mapOf("/own.js" to "1;") })
        openedBy(Origins(base, loopbackHttp = true), documents = false)
        waitForSaid(4)
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
            "</script>"

    @Test
    fun aSocketGoesOnlyToTheAppsOrigins() {
        documentChecks()
        serve({ socketsOfThePage() })
        openedBy(Origins(base, loopbackHttp = true))
        waitForSaid(3)
        settle("socket-asked.txt", "with the check at the start of every document")
        assertEquals("what the page was told", listOf("direct-refused", "constructor-refused", "own-made").sorted(), said.sorted())
        assertEquals("a socket reached another origin: $otherAsked", emptyList<String>(), otherAsked.toList())
        assertTrue("the page's own socket opens: $ownSockets", waitFor(5) { ownSockets.isNotEmpty() })
    }

    @Test
    fun aSocketReachesAnotherOriginWhereNothingChecksTheDocument() {
        serve({ socketsOfThePage() })
        openedBy(Origins(base, loopbackHttp = true), documents = false)
        waitForSaid(3)
        assertTrue("the socket reached the other origin: $otherAsked", waitFor(10) { otherAsked.any { it.startsWith("UPGRADE") } })
        settle("socket-control.txt", "without the check at the start of every document")
    }

    @Test
    fun whatASocketOpenedInsideAWorkerReachesIsRecorded() {
        documentChecks()
        serve(
            { "<p>ours</p><script>var w = new Worker('/socket-worker.js'); w.onmessage = function(e){ new Image().src = '/says?m=' + encodeURIComponent(e.data) }</script>" },
            { mapOf("/socket-worker.js" to "var s = new WebSocket('ws://localhost:${other.port}/socket'); s.onopen = function(){ postMessage('socket opened') }; s.onerror = function(){ postMessage('socket failed') }") },
        )
        openedBy(Origins(base, loopbackHttp = true))
        waitForSaid()
        settle("socket-in-worker.txt", "a socket opened inside a worker the page started from one of the app's addresses")
        assertTrue("the worker ran: $said", said.isNotEmpty())
    }
}
