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

    // ------------------------------------------------------------------ what else a service worker does

    // The service worker loads a script from another origin with importScripts and says what came of it to the
    // app's own server (a worker has no page to tell).
    private fun importingServiceWorker() =
        "try { importScripts('$elsewhere/import.js'); fetch('/says?m=sw-import-reached') } catch (e) { fetch('/says?m=sw-import-blocked') }"

    private val registersTheImporter = "<p>ours</p><script>navigator.serviceWorker.register('/sw.js')</script>"

    @Test
    fun aServiceWorkerNeverImportsAScriptFromAnotherOrigin() {
        serve({ registersTheImporter }, { mapOf("/sw.js" to importingServiceWorker()) })
        val origins = Origins(base, loopbackHttp = true)
        var inForce = false
        compose.runOnUiThread {
            WorkerGuard.arm(origins)
            inForce = WorkerGuard.ensure()
        }
        assertTrue("the rules for service workers are in force", inForce)
        openedBy(origins)
        waitForSaid()
        settle("worker-import.txt", "importScripts of another origin's script inside a service worker, with the guard")
        assertEquals("what the worker said", listOf("sw-import-blocked"), said.toList())
        assertEquals("the other origin was asked: $otherAsked", emptyList<String>(), otherAsked.toList())
    }

    @Test
    fun aServiceWorkerImportsAScriptFromAnotherOriginWhereNothingGuardsIt() {
        serve({ registersTheImporter }, { mapOf("/sw.js" to importingServiceWorker()) })
        compose.runOnUiThread {
            ServiceWorkerController.getInstance().setServiceWorkerClient(object : ServiceWorkerClient() {
                override fun shouldInterceptRequest(request: WebResourceRequest): WebResourceResponse? = null
            })
        }
        openedBy(Origins(base, loopbackHttp = true))
        waitForSaid()
        settle("worker-import-control.txt", "importScripts of another origin's script inside a service worker, no rules")
        assertEquals("what the worker said", listOf("sw-import-reached"), said.toList())
        assertTrue("the other origin was asked: $otherAsked", otherAsked.contains("GET /import.js"))
    }

    @Test
    fun aServiceWorkerWhoseScriptIsOnAnotherOriginIsNotRegistered() {
        serve({
            "<p>ours</p><script>navigator.serviceWorker.register('$elsewhere/sw.js').then(function(){ new Image().src = '/says?m=registered' }, function(e){ new Image().src = '/says?m=' + encodeURIComponent('register-failed ' + e.name) })</script>"
        })
        val origins = Origins(base, loopbackHttp = true)
        compose.runOnUiThread {
            WorkerGuard.arm(origins)
            WorkerGuard.ensure()
        }
        openedBy(origins)
        waitForSaid()
        settle("worker-register-elsewhere.txt", "a registration whose script is on another origin")
        assertEquals("what the page was told", listOf("register-failed SecurityError"), said.toList())
        assertEquals("the other origin was asked: $otherAsked", emptyList<String>(), otherAsked.toList())
    }

    // ------------------------------------------------------------------ other kinds of request a page makes

    /**
     * A kind of request a page can make, once to the app's own server and once to the other origin, with the
     * client's checks on: the other origin is never asked. The request to the app's own server is the sign that the
     * page made the request at all; where [mustArrive], the test waits for it, and then waits as long again for
     * anything to reach the other origin. What the client was asked about is written beside the pictures.
     */
    private fun kind(name: String, own: String, mustArrive: Boolean, page: () -> String, files: () -> Map<String, String> = { emptyMap() }) {
        serve(page, files)
        openedBy(Origins(base, loopbackHttp = true))
        val came = waitFor(15) { appAsked.any { it.endsWith(" $own") } }
        waitFor(10) { otherAsked.isNotEmpty() }
        settle("request-$name.txt", "$name, to the app's own server and to another origin, with the client's checks on; the app's own came: $came")
        if (mustArrive) assertTrue("the page made the request to the app's own server: $appAsked", came)
        assertEquals("the other origin was asked: $otherAsked", emptyList<String>(), otherAsked.toList())
    }

    @Test
    fun aBeaconGoesOnlyToTheAppsOrigins() = kind("beacon", "/beacon-own", mustArrive = true, page = {
        "<p>ours</p><script>navigator.sendBeacon('/beacon-own', 'Moga'); navigator.sendBeacon('$elsewhere/beacon', 'Moga')</script>"
    })

    @Test
    fun aFetchThatOutlivesThePageGoesOnlyToTheAppsOrigins() = kind("keepalive", "/keepalive-own", mustArrive = true, page = {
        "<p>ours</p><script>fetch('/keepalive-own', {method: 'POST', body: 'Moga', keepalive: true}).catch(function(){});" +
            "fetch('$elsewhere/keepalive', {method: 'POST', body: 'Moga', keepalive: true, mode: 'no-cors'}).catch(function(){})</script>"
    })

    @Test
    fun aLinksPingGoesOnlyToTheAppsOrigins() = kind("ping", "/ping-own", mustArrive = false, page = {
        "<p>ours</p><a id=\"a\" href=\"/pinged\" ping=\"/ping-own $elsewhere/ping\">Bathinda</a><script>document.getElementById('a').click()</script>"
    })

    @Test
    fun anEventStreamGoesOnlyToTheAppsOrigins() = kind("eventsource", "/events-own", mustArrive = true, page = {
        "<p>ours</p><script>new EventSource('/events-own'); new EventSource('$elsewhere/events')</script>"
    })

    @Test
    fun aPrefetchHintGoesOnlyToTheAppsOrigins() = kind("prefetch", "/prefetch-own", mustArrive = false, page = {
        "<link rel=\"prefetch\" href=\"/prefetch-own\"><link rel=\"prefetch\" href=\"$elsewhere/prefetch\"><p>ours</p>"
    })

    @Test
    fun aModuleWorkersImportsComeOnlyFromTheAppsOrigins() = kind(
        "module-worker",
        "/own-import.js",
        mustArrive = false,
        page = {
            "<p>ours</p><script>new Worker('/module.js', {type: 'module'}); new Worker('/module-dynamic.js', {type: 'module'})</script>"
        },
        files = {
            mapOf(
                "/module.js" to "import '/own-import.js'; import '$elsewhere/import.js';",
                "/module-dynamic.js" to "import('$elsewhere/dynamic.js').catch(function(){});",
                "/own-import.js" to "export default 1;",
            )
        },
    )

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

    // ------------------------------------------------------------------ a page that replaces what a check could use

    /**
     * What a page's script can replace after the check at the start of the document ran, one change to a frame
     * (each `srcdoc` frame is a window of its own and is checked as the page is): the global `URL` and `String`,
     * the string and regular expression methods, the getters of `URL.prototype`, `Reflect` and `Function.prototype`'s
     * `call` and `apply`, `Array.from` and the array methods. Each frame then opens the app's own socket, tries
     * another server's, and hands over an object that answers the app's address the first time it is turned into a
     * string and the other server's the second. Two more frames do the same for workers, with a `blob:` address.
     */
    private fun pageThatReplacesWhatTheCheckCouldUse(): String {
        val own = "ws://localhost:${app.port}/own-socket"
        val otherSocket = "ws://localhost:${other.port}/socket"
        val ownHttp = "http://localhost:${app.port}/"
        val blob = "URL_ORIGINAL.createObjectURL(new Blob(['${socketOfTheWorker().replace("'", "\\'")}'], {type: 'text/javascript'}))"
        val changes = linkedMapOf(
            "url" to "URL = function(){ return {href: '$ownHttp', protocol: 'http:', hostname: 'localhost', port: '${app.port}'} };",
            "string" to "String = function(u){ return u };",
            "strings" to "String.prototype.replace = function(){ return '$ownHttp' }; String.prototype.slice = function(){ return 'x' }; String.prototype.indexOf = function(){ return 0 }; " +
                "String.prototype.endsWith = function(){ return true }; String.prototype.startsWith = function(){ return true }; RegExp.prototype[Symbol.replace] = function(){ return '$ownHttp' };",
            "getters" to "['href', 'protocol', 'hostname', 'port', 'host', 'origin'].forEach(function(n){ Object.defineProperty(URL.prototype, n, {configurable: true, get: function(){ return {href: '$ownHttp', protocol: 'http:', hostname: 'localhost', port: '${app.port}', host: 'localhost:${app.port}', origin: 'http://localhost:${app.port}'}[n] }}) });",
            "reflect" to "Reflect.apply = function(){ return '$ownHttp' }; Reflect.construct = function(){ return {href: '$ownHttp'} }; Function.prototype.call = function(){ return '$ownHttp' }; Function.prototype.apply = function(){ return '$ownHttp' };",
            "arrays" to "Array.from = function(){ return [] }; Array.prototype.forEach = function(){}; Array.prototype.indexOf = function(){ return 0 }; Array.prototype.includes = function(){ return true };",
            "primitive" to "",
        )
        fun frame(name: String, script: String) =
            "<iframe srcdoc=\"" + ("<script>var I = Image, URL_ORIGINAL = URL, R = function(m){ new I().src = '/says?m=$name-' + m };$script</script>")
                .replace("&", "&amp;").replace("\"", "&quot;") + "\"></iframe>"
        fun attempt(what: String, body: String) = "try { $body; R('$what-made') } catch (e) { R('$what-refused') }"
        val frames = StringBuilder()
        for ((name, change) in changes) {
            val turned = if (name == "primitive") "{[Symbol.toPrimitive]: function(){ return n++ ? '$otherSocket' : '$own' }}" else "{toString: function(){ return n++ ? '$otherSocket' : '$own' }}"
            frames.append(frame(name, "try { $change } catch (e) {}" + attempt("own", "new WebSocket('$own')") + attempt("other", "new WebSocket('$otherSocket')") + "var n = 0;" + attempt("object", "new WebSocket($turned)")))
        }
        for ((name, change) in listOf("worker-url" to changes.getValue("url"), "worker-string" to changes.getValue("string"))) {
            frames.append(frame(name, "var b = $blob; try { $change } catch (e) {}" + attempt("own", "new Worker('/own.js')") + attempt("blob", "new Worker(b)") + "var n = 0;" + attempt("object", "new Worker({toString: function(){ return n++ ? b : '/own.js' }})")))
        }
        // A socket of the newer kind, where the web view has it: checked as WebSocket is.
        val stream = "<script>if (typeof WebSocketStream === 'function') { new Image().src = '/says?m=stream-present'; try { new WebSocketStream('$otherSocket'); new Image().src = '/says?m=stream-other-made' } catch (e) { new Image().src = '/says?m=stream-other-refused' } } else { new Image().src = '/says?m=stream-absent' }</script>"
        return "<p>ours</p>$frames$stream"
    }

    @Test
    fun aPageThatReplacesWhatACheckCouldUseStillOpensASocketOrStartsAWorkerOnlyOnTheAppsOrigins() {
        documentChecks()
        serve({ pageThatReplacesWhatTheCheckCouldUse() }, { mapOf("/own.js" to "1;") })
        openedBy(Origins(base, loopbackHttp = true))
        val sockets = listOf("url", "string", "strings", "getters", "reflect", "arrays", "primitive")
        val expected = sockets.flatMap { listOf("$it-own-made", "$it-other-refused", "$it-object-made") } +
            listOf("worker-url", "worker-string").flatMap { listOf("$it-own-made", "$it-blob-refused", "$it-object-made") }
        waitForSaid(expected.size + 1, seconds = 45)
        // A blob worker that ran would open its socket some time after it was made: as long as the control waits for it.
        waitFor(15) { otherAsked.isNotEmpty() }
        settle("socket-replaced.txt", "frames whose scripts replace globals, prototype methods and getters after the check ran")
        assertEquals("what the frames were told", expected.sorted(), said.filter { !it.startsWith("stream-") }.sorted())
        val stream = said.filter { it.startsWith("stream-") }
        assertTrue("a WebSocketStream, where there is one, is refused another server: $stream", stream == listOf("stream-absent") || stream.sorted() == listOf("stream-other-refused", "stream-present"))
        assertEquals("a socket or a worker reached another origin: $otherAsked", emptyList<String>(), otherAsked.toList())
        assertTrue("the app's own sockets open: $ownSockets", waitFor(5) { ownSockets.isNotEmpty() })
    }

    // A host the app names for the web view is one a page's socket may go to, as its loads may; a host it does not
    // name is refused. The other server here is the named host; the unnamed one is the same server by its address.
    @Test
    fun aSocketGoesToAHostTheAppNamesForTheWebViewAndNoOther() {
        documentChecks()
        serve({
            "<p>ours</p><script>" +
                madeOrRefused("named", "var s = new WebSocket('ws://localhost:${other.port}/socket')") +
                madeOrRefused("unnamed", "new WebSocket('ws://127.0.0.1:${other.port}/socket')") +
                "</script>"
        })
        openedBy(Origins(base, loopbackHttp = true, webHosts = listOf("localhost:${other.port}")))
        waitForSaid(2)
        assertTrue("the named host's socket reached it: $otherAsked", waitFor(10) { otherAsked.contains("UPGRADE /socket") })
        settle("socket-web-host.txt", "a socket to a host in webHosts, and one to an address that is not")
        assertEquals("what the page was told", listOf("named-made", "unnamed-refused"), said.sorted())
        assertEquals("only the named host's socket reached the server: $otherAsked", listOf("UPGRADE /socket"), otherAsked.toList())
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
    // does not get the script at the start of a document, so a socket or a worker made through it is not checked,
    // and a worker from a blob: address made there runs and opens its own socket. A frame given a document
    // (srcdoc, or an address) is checked.
    @Test
    fun aSocketOrAWorkerMadeFromAnEmptyFrameThePageMakesIsNotChecked() {
        documentChecks()
        val workerSocket = "new WebSocket('ws://localhost:${other.port}/from-a-worker')"
        serve({
            "<p>ours</p><script>" +
                "var f = document.createElement('iframe'); document.body.appendChild(f);" +
                madeOrRefused("empty-frame", "new f.contentWindow.WebSocket('ws://localhost:${other.port}/socket')") +
                madeOrRefused("empty-frame-worker", "new f.contentWindow.Worker(URL.createObjectURL(new Blob([\"$workerSocket\"], {type: 'text/javascript'})))") +
                "</script><iframe srcdoc=\"<script>try { new WebSocket('ws://localhost:${other.port}/socket'); new Image().src = '/says?m=srcdoc-made' } catch (e) { new Image().src = '/says?m=srcdoc-refused' }</script>\"></iframe>"
        })
        openedBy(Origins(base, loopbackHttp = true))
        waitForSaid(3)
        waitFor(15) { otherAsked.size >= 2 }
        settle("socket-in-frame.txt", "a socket and a blob worker made from a frame the page made, an empty one and one with srcdoc")
        assertEquals("what the frames were told", listOf("empty-frame-made", "empty-frame-worker-made", "srcdoc-refused"), said.sorted())
        assertEquals(
            "only the empty frame's socket and its worker's socket reached the other origin: $otherAsked",
            listOf("UPGRADE /from-a-worker", "UPGRADE /socket"),
            otherAsked.sorted(),
        )
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
