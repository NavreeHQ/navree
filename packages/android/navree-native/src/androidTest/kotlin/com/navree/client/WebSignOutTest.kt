package com.navree.client

import android.webkit.CookieManager
import android.webkit.ServiceWorkerClient
import android.webkit.ServiceWorkerController
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.viewinterop.AndroidView
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.webkit.WebViewFeature
import com.navree.client.core.Body
import com.navree.client.core.Driver
import com.navree.client.core.Effect
import com.navree.client.core.Engine
import com.navree.client.platform.AndroidEffects
import com.navree.client.platform.OriginWebClient
import com.navree.client.platform.Origins
import com.navree.client.platform.OwedToken
import com.navree.client.platform.PushProvider
import com.navree.client.platform.SiteWipe
import com.navree.client.platform.WipeRules
import com.navree.client.platform.guardDocuments
import com.navree.client.platform.guardWorkers
import com.navree.protocol.Navree
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonPrimitive
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/**
 * What a page of the app stores in the web view, and what a page shown after
 * the person signs out finds of it. One page stores a cookie, a value in local
 * storage, one in IndexedDB and one in Cache Storage, and registers a service
 * worker; the session ends through the client's own sign-out (the core's
 * `forget_session` effect, carried out by [AndroidEffects]); then a page of
 * the same origin in a new web view says what it finds: nothing, on a web view
 * that deletes every kind of site data itself and on one that does not, where
 * the client's own document on the origin takes the worker and the cache away
 * (the file beside the pictures says which this web view is). The control is
 * the same without the sign-out: the second page finds all five, so the page
 * that looks can see them. And a worker that answers every page of its origin
 * itself, the client's own document included, is gone as well.
 */
@RunWith(AndroidJUnit4::class)
class WebSignOutTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val app = MockWebServer()

    /** What the pages told the app's server, in order. */
    private val said = CopyOnWriteArrayList<String>()

    /** The path of every request the app's server was sent, in order. */
    private val asked = CopyOnWriteArrayList<String>()

    /** Whether the server sends a request for a path it does not know to its sign-in page; else it answers such a request with nothing. */
    private var visitor = false

    /** Where the push token of this device is removed, and what holds the server's answer to it back: a test that has it asked lets go at its end. */
    private val removal = "/api/notifications/push-devices"
    private val unanswered = CountDownLatch(1)

    /** The `Cookie` header the removal came with. */
    @Volatile
    private var removalCookie: String? = null

    /**
     * A push service of the test's own: it keeps each deletion of the token it is asked for, and answers one only when
     * the test has it answer, so the time between the client being told and the service's answer is the test's to hold open.
     */
    private class Pushes : PushProvider {
        override val provider = "fcm"

        val asked = CopyOnWriteArrayList<(Boolean) -> Unit>()

        override fun token(done: (String?) -> Unit) = done(null)

        override fun forget(done: (gone: Boolean) -> Unit) {
            asked += done
        }
    }

    @After
    fun stop() {
        app.close()
        // A deletion a test here left unfinished has failed that test; it does not go on to refuse the web pages of the tests after it.
        SiteWipe.forget()
        // Nor does a push token a test left owed hold back what a later test shows.
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        OwedToken.of(context).gone()
        // Nor is the person a test named here remembered by a later one.
        File(context.noBackupFilesDir, "navree-person").delete()
    }

    private fun page(script: String) = MockResponse.Builder()
        .code(200)
        .addHeader("Content-Type", "text/html; charset=utf-8")
        .body("<!doctype html><html><head><meta charset=\"utf-8\"></head><body><p>ours</p><script>var R = function(m){ new Image().src = '/says?m=' + encodeURIComponent(m) };$script</script></body></html>")
        .build()

    private val stores =
        "document.cookie = 'city=Amritsar; max-age=3600; path=/';" +
            "localStorage.setItem('city', 'Ludhiana');" +
            "var open = indexedDB.open('orders', 1);" +
            "open.onupgradeneeded = function(){ open.result.createObjectStore('o') };" +
            "open.onerror = function(){ R('failed indexeddb') };" +
            "open.onsuccess = function(){ var t = open.result.transaction('o', 'readwrite'); t.objectStore('o').put('Patiala', 'first');" +
            " t.oncomplete = function(){ open.result.close();" +
            "  caches.open('pages').then(function(c){ return c.put('/kept', new Response('Jalandhar')) })" +
            "  .then(function(){ return navigator.serviceWorker.register('/sw.js') })" +
            "  .then(function(){ return navigator.serviceWorker.ready })" +
            "  .then(function(){ R('stored') }, function(e){ R('failed ' + e) }) } };"

    private val looks =
        "Promise.all([" +
            "indexedDB.databases ? indexedDB.databases().then(function(d){ return d.map(function(x){ return x.name }) }) : Promise.resolve(['no list']), " +
            "caches.keys(), " +
            "navigator.serviceWorker.getRegistrations().then(function(r){ return r.length })" +
            "]).then(function(a){ R(JSON.stringify({cookie: document.cookie, local: localStorage.length, indexeddb: a[0], caches: a[1], workers: a[2]})) }, function(e){ R('failed ' + e) });"

    private lateinit var base: String

    /** The service worker the first page registers: one that leaves every request to the network, unless a test names another. */
    private var worker = "self.addEventListener('fetch', function(){});"

    /**
     * A worker that takes its origin's pages over at once and answers every
     * one of them itself, with a page of its own that says so: while it is
     * registered, no page of the server's is shown on its origin.
     */
    private val answering = """
        self.addEventListener('install', function(){ self.skipWaiting() });
        self.addEventListener('activate', function(e){ e.waitUntil(self.clients.claim()) });
        self.addEventListener('fetch', function(e){
          if (e.request.mode === 'navigate') e.respondWith(new Response(
            '<!doctype html><html><body><script>new Image().src = "/says?m=" + encodeURIComponent("answered by the worker")</script></body></html>',
            {headers: {'Content-Type': 'text/html; charset=utf-8'}}));
        });
    """.trimIndent()

    /**
     * A worker that takes its origin's pages over at once and asks the
     * network for every request of theirs, the pages themselves included.
     */
    private val fetching = """
        self.addEventListener('install', function(){ self.skipWaiting() });
        self.addEventListener('activate', function(e){ e.waitUntil(self.clients.claim()) });
        self.addEventListener('fetch', function(e){ e.respondWith(fetch(e.request)) });
    """.trimIndent()

    /**
     * A worker that takes its origin's pages over at once and, each time a
     * page of the origin is asked for, sends a write of its own to the app's
     * server and says which page it was for: as a worker does that sends a
     * queue of writes again whenever it is started. It also opens a socket to
     * the app's server, which is no request the web view asks the client
     * about, and writes a cookie of its own, where the web view lets a worker
     * do either. The page itself it leaves to the network.
     */
    private val replaying = """
        self.addEventListener('install', function(){ self.skipWaiting() });
        self.addEventListener('activate', function(e){ e.waitUntil(self.clients.claim()) });
        self.addEventListener('fetch', function(e){
          if (e.request.mode !== 'navigate') return;
          var page = encodeURIComponent(new URL(e.request.url).pathname);
          fetch('/replayed?for=' + page, {method: 'POST', credentials: 'include'}).catch(function(){});
          try { new WebSocket('ws://' + self.location.host + '/socket?for=' + page).onerror = function(){} } catch (x) {}
          try { if (self.cookieStore) self.cookieStore.set('worker', 'wrote').catch(function(){}) } catch (x) {}
        });
    """.trimIndent()

    /** The page each write of the [replaying] worker that reached the app's server was for, in order. */
    private val replayed = CopyOnWriteArrayList<String>()

    /** The page each socket of the [replaying] worker that reached the app's server was opened for, with the cookies it came with. */
    private val sockets = CopyOnWriteArrayList<Pair<String, String?>>()

    private fun serve() {
        app.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse = when (request.url.encodedPath.also { asked += it }) {
                "/says" -> {
                    said += request.url.queryParameter("m") ?: ""
                    MockResponse.Builder().code(204).build()
                }
                "/" -> page(stores)
                // To the storing page on the app's second origin: the same server by its address.
                "/enter" -> MockResponse.Builder().code(302).addHeader("Location", "http://127.0.0.1:${request.url.port}/").build()
                "/look" -> page(looks)
                "/sw.js" -> MockResponse.Builder().code(200).addHeader("Content-Type", "text/javascript").body(worker).build()
                "/sign-in" -> page("")
                "/replayed" -> {
                    replayed += request.url.queryParameter("for") ?: ""
                    MockResponse.Builder().code(204).build()
                }
                // A socket's first request: it is refused, after what it came with is written down.
                "/socket" -> {
                    sockets += (request.url.queryParameter("for") ?: "") to request.headers["Cookie"]
                    MockResponse.Builder().code(404).build()
                }
                // What a sign-in's answer does: it sets the cookie of the session of the person who signed in.
                "/session" -> MockResponse.Builder().code(204).addHeader("Set-Cookie", "session=Karan; Path=/").build()
                removal -> {
                    removalCookie = request.headers["Cookie"]
                    unanswered.await(30, TimeUnit.SECONDS)
                    MockResponse.Builder().code(204).build()
                }
                else -> if (visitor) MockResponse.Builder().code(302).addHeader("Location", "/sign-in").build() else MockResponse.Builder().code(204).build()
            }
        }
        app.start()
        base = app.url("/").toString().trimEnd('/')
    }

    /** The app's second origin: the same server by its address, which the web view keeps apart from the first. */
    private fun second() = "http://127.0.0.1:${app.port}"

    private var shown by mutableStateOf<String?>(null)

    /** One web view at a time, made new for each address shown: as a web page of the app is. */
    private fun show(address: String) {
        compose.runOnUiThread { shown = address }
    }

    /**
     * Waits up to [seconds] for [what], letting the composition draw meanwhile: the test's clock moves only while the
     * test waits through the rule, so a web view a state change brings in is made only then.
     */
    private fun waitFor(seconds: Int, what: () -> Boolean): Boolean = try {
        compose.waitUntil(seconds * 1_000L) { what() }
        true
    } catch (_: Throwable) {
        what()
    }

    /**
     * Shows the page at [path] and waits until it has stored all five. With
     * [origins], the web view has the client's own checks for them, as a web
     * page of the app has, and the client is told which origins it is for;
     * without, the web view's client is the system's own. With [guard], a
     * service worker's requests are asked of the app's own rules from the
     * start, as in the app; without, of nobody until the session ends.
     */
    private fun start(path: String = "/", origins: (() -> Origins)? = null, guard: Boolean = false) {
        serve()
        val checked = origins?.invoke()
        if (checked != null) compose.runOnUiThread { SiteWipe.arm(compose.activity, checked) }
        if (guard) {
            val set = compose.runOnUiThread { guardWorkers(Origins(base, loopbackHttp = true)) }
            assertTrue("the rules for a service worker's requests were set", set)
        } else {
            // The service worker's script comes from the app's own origin, and the test is not about the rules for its requests.
            compose.runOnUiThread {
                ServiceWorkerController.getInstance().setServiceWorkerClient(object : ServiceWorkerClient() {
                    override fun shouldInterceptRequest(request: WebResourceRequest): WebResourceResponse? = null
                })
            }
        }
        compose.setContent {
            val address = shown
            if (address != null) {
                key(address) {
                    AndroidView(
                        modifier = Modifier.fillMaxSize(),
                        factory = { context ->
                            WebView(context).apply {
                                settings.javaScriptEnabled = true
                                settings.domStorageEnabled = true
                                if (checked != null) {
                                    if (!guardDocuments(this, checked)) said += "the check at the start of every document was not added"
                                    webViewClient = OriginWebClient(checked, onLink = {})
                                } else {
                                    webViewClient = WebViewClient()
                                }
                                loadUrl(address)
                            }
                        },
                        onRelease = { it.destroy() },
                    )
                }
            }
        }
        show("$base$path")
        assertTrue("the first page stored all five: $said", waitFor(30) { said.isNotEmpty() })
        assertEquals("the first page stored all five", listOf("stored"), said.toList())
        // The page that stored them is left, as a person leaves it before signing out.
        compose.runOnUiThread { shown = null }
        compose.waitForIdle()
    }

    /** What the page shown next finds, read from what it said. */
    private fun looked(file: String, what: String, address: String = "$base/look"): JsonObject {
        val before = said.size
        show(address)
        assertTrue("the second page said what it found: $said", waitFor(30) { said.size > before })
        val found = said.last()
        Shots.put(file, "$what\nthe system's web view deletes every kind of site data: ${WebViewFeature.isFeatureSupported(WebViewFeature.DELETE_BROWSING_DATA)}\nthe second page found: $found\n")
        android.util.Log.i("WebSignOutTest", "$file: $found")
        assertTrue("the second page could look: $found", found.startsWith("{"))
        return Json.parseToJsonElement(found) as JsonObject
    }

    /**
     * The control, and what a change of person takes. A page shown after
     * another finds all five while nobody signed out and the same person is
     * signed in. Then the core names another person, as it does when a
     * session ran out and somebody else signed in, and the client is told of
     * no end of a session. A page shown after that finds nothing of what the
     * pages of the person before stored, and not the cookie their page's
     * script set either: the one cookie store was emptied. What it does find
     * is the cookie an answer to one of the client's own requests set, as a
     * sign-in's answer does, which was given back, so the person who is here
     * now is still signed in.
     *
     * The worker here sends a write of its own to the app's server each time
     * a page of its origin is asked for. It does so for the page that looks,
     * while nobody's things are being taken away. The deletion's own
     * document is a page of that origin too, and it starts the worker of the
     * person before: what the worker sends then never reaches the server,
     * where it would have arrived with the new person's cookie. A socket the
     * worker opens is not a request the client is asked about, so it does
     * reach the server where the web view lets a worker open one: it comes
     * with no cookie, since the store is emptied for as long as the deletion
     * runs. And a cookie the worker writes while it runs is gone afterwards,
     * with the store's second emptying.
     *
     * The store's queue is held busy when the core names the second person:
     * for as long as it is, nothing of the cookies has been done, and what
     * pages stored stays owed, so no page is shown.
     */
    @Test
    fun aPageShownAfterAnotherFindsWhatItStoredWhenNobodySignedOutAndOnlyTheCookiesOfTheClientsOwnAnswersOnceAnotherPersonIsSignedIn() {
        worker = replaying
        start()
        val found = looked("web-storage-kept.txt", "no sign-out between the two pages")
        assertTrue("the cookie: $found", found.getValue("cookie").jsonPrimitive.content.contains("city=Amritsar"))
        assertEquals("local storage: $found", 1, found.getValue("local").jsonPrimitive.int)
        assertTrue("IndexedDB: $found", found.getValue("indexeddb").jsonArray.any { it.jsonPrimitive.content == "orders" })
        assertTrue("Cache Storage: $found", found.getValue("caches").jsonArray.any { it.jsonPrimitive.content == "pages" })
        assertEquals("a service worker: $found", 1, found.getValue("workers").jsonPrimitive.int)
        // The control for the worker: with nothing being taken away, what it sends for the page that looked reaches the server.
        assertTrue("the worker's own write for the page that looked reached the server: $replayed", waitFor(15) { replayed.contains("/look") })

        // The page that looked is left. The core names one person; then an answer to one of the client's own requests sets a
        // session's cookie, as the sign-in of the next person does; then the core names that person, with no word of a sign-out.
        compose.runOnUiThread { shown = null }
        compose.waitForIdle()
        val driver = Driver(object : Engine {
            override fun send(event: ByteArray): ByteArray = "{}".encodeToByteArray()

            override fun view(query: ByteArray): ByteArray = "{}".encodeToByteArray()
        })
        lateinit var effects: AndroidEffects
        compose.runOnUiThread {
            effects = AndroidEffects(compose.activity, driver, "$base/".toHttpUrl(), null, null, Origins(base, loopbackHttp = true), loopbackHttp = true)
            effects.hears(Navree.User(name = "Anjali Verma", email = "anjali.verma@example.com"))
            effects.run(Effect.Fetch(2, "get", "$base/session", emptyList(), null, 5_000, false))
        }
        val cookies = CookieManager.getInstance()
        assertTrue("the answer's cookie reached the store: ${cookies.getCookie(base)}", waitFor(15) { cookies.getCookie(base).orEmpty().contains("session=Karan") })
        assertTrue("beside the one the page's script set: ${cookies.getCookie(base)}", cookies.getCookie(base).orEmpty().contains("city=Amritsar"))
        // The store's queue is busy with what was asked of it before, for as long as the test holds it.
        val busy = CountDownLatch(1)
        effects.state.always { busy.await(40, TimeUnit.SECONDS) }
        try {
            compose.runOnUiThread { effects.hears(Navree.User(name = "Karan Malhotra", email = "karan.malhotra@example.com")) }
            assertTrue("what the pages of the person before stored is owed the moment the core names another person", SiteWipe.owes)
            assertFalse("and stays owed while the store's queue is busy: no page is shown before the cookies have had their turn", waitFor(2) { !SiteWipe.owes })
            assertTrue("whose turn has not come: ${cookies.getCookie(base)}", cookies.getCookie(base).orEmpty().contains("city=Amritsar"))
        } finally {
            busy.countDown()
        }
        val turned = AtomicBoolean(false)
        effects.state.always { turned.set(true) }
        assertTrue("the change of person had its turn in the store", waitFor(30) { turned.get() })
        assertTrue("what the pages of the person before stored is gone, so nothing is owed any more", waitFor(60) { !SiteWipe.owes })
        compose.runOnUiThread { effects.close() }

        val after = looked("web-storage-after-another-person.txt", "another person was signed in, with no sign-out between\nthe sockets the worker opened, and the cookies each came with: $sockets")
        val left = after.getValue("cookie").jsonPrimitive.content
        assertFalse("the cookie the page's script set for the person before is gone: $after", left.contains("city=Amritsar"))
        assertTrue("the cookie the client's own answer set was given back: $after", left.contains("session=Karan"))
        assertFalse("a cookie the worker wrote while its origin was being cleared is gone: $after", left.contains("worker="))
        assertEquals("nothing in local storage: $after", 0, after.getValue("local").jsonPrimitive.int)
        assertEquals("no IndexedDB database: $after", 0, after.getValue("indexeddb").jsonArray.size)
        assertEquals("nothing in Cache Storage: $after", 0, after.getValue("caches").jsonArray.size)
        assertEquals("no service worker: $after", 0, after.getValue("workers").jsonPrimitive.int)
        assertEquals("what the worker sent while its origin was being cleared never reached the server: $replayed", listOf("/look"), replayed.distinct())
        // A socket is no request the client is asked about. One the worker opened while its origin was being cleared came with
        // nobody's session and nothing a page had kept: at most with the cookie the worker itself had just written.
        val carried = sockets.filter { it.first != "/look" }.mapNotNull { it.second }.filter { it.contains("session=") || it.contains("city=") }
        assertEquals("a socket opened while the origin was being cleared carried no session and no cookie of a page: $sockets", emptyList<String>(), carried)
    }

    /**
     * Ends the session through the client's own sign-out, and waits until the
     * web view has said that what its pages stored is gone: until then a
     * deletion is owed, and the app shows no web page.
     */
    private fun signOut() = client { it.run(Effect.ForgetSession) }

    /**
     * A client of the app made now, as at a start of the app, and [then] with
     * it; waits until whatever it owes of the end of a session is done, and
     * then [after] with it. With [push], the app has that push service.
     */
    private fun client(push: PushProvider? = null, after: (AndroidEffects) -> Unit = {}, then: (AndroidEffects) -> Unit) {
        val driver = Driver(object : Engine {
            override fun send(event: ByteArray): ByteArray = "{}".encodeToByteArray()

            override fun view(query: ByteArray): ByteArray = "{}".encodeToByteArray()
        })
        lateinit var effects: AndroidEffects
        compose.runOnUiThread {
            effects = AndroidEffects(compose.activity, driver, "$base/".toHttpUrl(), null, push, Origins(base, loopbackHttp = true), loopbackHttp = true)
            then(effects)
        }
        // Whatever the store does after the end of the session runs only once the wipe is done, or its time limit has passed.
        val wiped = AtomicBoolean(false)
        effects.state.always { wiped.set(true) }
        assertTrue("the end of the session finished", waitFor(30) { wiped.get() })
        assertTrue("the web view said that what its pages stored is gone, so nothing is owed any more", waitFor(60) { !SiteWipe.owes })
        compose.runOnUiThread {
            after(effects)
            effects.close()
        }
    }

    /** Nothing of the five is left, whichever way this web view deletes: the call that deletes every kind, or the older call and the client's own document. */
    private fun assertNothingLeft(found: JsonObject) {
        assertEquals("no cookie: $found", "", found.getValue("cookie").jsonPrimitive.content)
        assertEquals("nothing in local storage: $found", 0, found.getValue("local").jsonPrimitive.int)
        assertEquals("no IndexedDB database: $found", 0, found.getValue("indexeddb").jsonArray.size)
        assertEquals("nothing in Cache Storage: $found", 0, found.getValue("caches").jsonArray.size)
        assertEquals("no service worker: $found", 0, found.getValue("workers").jsonPrimitive.int)
    }

    @Test
    fun aPageShownAfterTheSessionEndedFindsNothingAnEarlierPageStored() {
        start()
        signOut()
        assertNothingLeft(looked("web-storage-after-sign-out.txt", "the session ended between the two pages"))
    }

    @Test
    fun aWorkerThatAnswersItsOriginsPagesItselfIsGoneAfterTheSessionEnded() {
        worker = answering
        start()
        // The control: while the worker is registered, a page of its origin is the worker's own, not the server's.
        val before = said.size
        show("$base/look")
        assertTrue("a page shown while the worker is there said something: $said", waitFor(30) { said.size > before })
        assertEquals("the page shown while the worker is there is the worker's own", "answered by the worker", said.last())
        compose.runOnUiThread { shown = null }
        compose.waitForIdle()
        signOut()
        // The client's own document on this origin was answered by the worker too, and still took it away.
        assertNothingLeft(looked("web-worker-after-sign-out.txt", "the session ended; the worker answered every page of its origin itself"))
        assertEquals("no page after the end of the session was the worker's: $said", 1, said.count { it == "answered by the worker" })
    }

    /**
     * A page reached only through a redirect, on an origin the client knows
     * of no other way. The app's first origin answers `/enter` with a redirect
     * to a second origin of the app, and the page there stores what the first
     * test's page does. The session is then ended by a client that names only
     * the first origin, as an app does whose second origin is a host under a
     * `*.` pattern, which nobody can list. What the page stored on the second
     * origin is gone all the same: the web view's client wrote that origin
     * down when the page went there. (The second origin is the same server by
     * its address: plain `http` is an origin of the app only for the machine
     * itself, never under a pattern.)
     */
    @Test
    fun anOriginAPageReachedOnlyThroughARedirectIsClearedWhenTheSessionEnds() {
        start(path = "/enter", origins = { Origins(base, also = listOf(second()), loopbackHttp = true) })
        signOut()
        assertNothingLeft(looked("web-storage-after-a-redirect.txt", "the session ended; the page that stored was reached through a redirect to ${second()}", address = second() + "/look"))
    }

    /**
     * A worker that asks the network for every page of its origin, as an
     * app's own worker commonly does, on a server that sends a visitor it
     * does not know to its sign-in page whatever the address. The client's
     * own document is a page of that origin too: the worker's request for it
     * is answered by the client where a service worker's requests are asked,
     * so the worker hands the client's document on, the worker and the cache
     * are gone, and the server is never asked for that address.
     */
    @Test
    fun aWorkerThatFetchesEveryPageOfItsOriginIsGoneAfterTheSessionEndedAndTheServerIsNotAskedForTheClientsDocument() {
        worker = fetching
        visitor = true
        start(guard = true)
        signOut()
        assertNothingLeft(looked("web-fetching-worker-after-sign-out.txt", "the session ended; the worker fetched every page of its origin from the network"))
        val known = setOf("/", "/look", "/sw.js", "/says", "/sign-in", "/favicon.ico")
        assertEquals("the server was asked for nothing but its own pages: $asked", emptyList<String>(), asked.filter { it !in known })
    }

    /**
     * A run of the app that was told the session ended, wrote that down, and
     * ended before it got as far as the cookies: the next start of the app
     * does the whole of the end of a session, so a page shown after it finds
     * no cookie either, nor anything else the first page stored.
     */
    @Test
    fun theEndOfASessionThatARunOfTheAppDidNotFinishIsDoneWhollyByTheNextStart() {
        start()
        // What such a run leaves on the device: the mark, and everything the session's pages stored.
        File(compose.activity.noBackupFilesDir, "navree-web-wipe").writeText(WipeRules.EVERYTHING + "\n")
        client {}
        assertNothingLeft(looked("web-storage-after-an-unfinished-sign-out.txt", "a run of the app ended before the end of the session was done; the app started again"))
    }

    /**
     * The end of a session is owed from the moment the client is told of it.
     * The mark is on the disk when the effect returns, while the store is
     * still busy with what was asked before and nothing has been deleted: a
     * run of the app that ends there leaves the next start to do all of it
     * (the test before this one).
     */
    @Test
    fun theEndOfASessionIsWrittenDownTheMomentTheClientIsToldOfItBeforeAnythingIsWaitedFor() {
        serve()
        val mark = File(compose.activity.noBackupFilesDir, "navree-web-wipe")
        val busy = CountDownLatch(1)
        var written: String? = null
        var whole = false
        client {
            // The store is held by what was asked of it before: the turn of the end of the session has not come.
            it.state.always { busy.await(20, TimeUnit.SECONDS) }
            it.run(Effect.ForgetSession)
            written = runCatching { mark.readText() }.getOrNull()
            whole = SiteWipe.owesAll
            busy.countDown()
        }
        assertEquals("the mark was on the disk when the effect returned", WipeRules.EVERYTHING + "\n", written)
        assertTrue("and it owed the whole of the end of the session", whole)
    }

    /**
     * The update in which the core says the session is ending: the request
     * that removes the push token first, then the word. The server here does
     * not answer the removal. The mark is on the disk when the word's effect
     * returns; the removal was sent with the session's cookie, since its turn
     * in the store came first; and the whole of the end of the session is
     * done, the cookie gone and nothing owed, while the removal is still
     * unanswered. When the core then says the session is forgotten, nothing
     * is begun a second time.
     *
     * The push service's own token is asked to go at the same moment, the
     * removal unanswered, and its deletion is on the disk as owed when the
     * effect returns; the session forgotten afterwards asks the push service
     * no second time. The push service here never answers that run of the
     * app: the token stays owed when everything else is done, and a client
     * made afterwards, as the next start of the app makes one, asks the push
     * service again; when that is answered, nothing is owed.
     */
    @Test
    fun theEndOfASessionBeginsWhenTheCoreSaysItIsEndingAndDoesNotWaitForThePushTokensRemoval() {
        serve()
        val cookies = CookieManager.getInstance()
        cookies.setCookie(base, "session=Gurdaspur; path=/")
        val mark = File(compose.activity.noBackupFilesDir, "navree-web-wipe")
        val tokenMark = File(compose.activity.noBackupFilesDir, "navree-push-token")
        val service = Pushes()
        var written: String? = null
        var tokenOwed = false
        var askedAtOnce = -1
        var askedAfter = -1
        var owedAgain = true
        try {
            client(
                push = service,
                then = {
                    it.run(Effect.Fetch(1, "delete", "$base$removal", listOf("content-type" to "application/json"), Body.Json("{}"), 5_000, false))
                    it.run(Effect.SessionEnding)
                    written = runCatching { mark.readText() }.getOrNull()
                    tokenOwed = tokenMark.isFile
                    askedAtOnce = service.asked.size
                },
                after = {
                    it.run(Effect.ForgetSession)
                    owedAgain = SiteWipe.owes
                    askedAfter = service.asked.size
                },
            )
            assertEquals("the mark was on the disk when the effect returned", WipeRules.EVERYTHING + "\n", written)
            assertEquals("the removal is still unanswered", 1L, unanswered.count)
            assertEquals("the removal was sent with the session's cookie", "session=Gurdaspur", removalCookie)
            assertTrue("no cookie is left: ${cookies.getCookie(base)}", cookies.getCookie(base).isNullOrEmpty())
            assertFalse("the session forgotten after it was ending begins nothing again", owedAgain)
            assertTrue("the push token's deletion was on the disk as owed when the effect returned", tokenOwed)
            assertEquals("the push service was asked to delete the token when the core said the session is ending", 1, askedAtOnce)
            assertEquals("the session forgotten after it was ending asked the push service no second time", 1, askedAfter)
            // The push service did not answer, and that client is closed: the token is owed still, and the next start asks again.
            assertTrue("the token is still owed when the rest of the end of the session is done", tokenMark.isFile)
            val next = Pushes()
            client(push = next) {}
            assertEquals("the next start of the app asked the push service to delete the token", 1, next.asked.size)
            next.asked.single()(true)
            assertTrue("once the push service says the token is gone, nothing is owed", waitFor(15) { !tokenMark.exists() })
        } finally {
            unanswered.countDown()
        }
    }
}
