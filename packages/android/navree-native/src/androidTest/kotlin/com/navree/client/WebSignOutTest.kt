package com.navree.client

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
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.webkit.WebViewFeature
import com.navree.client.core.Driver
import com.navree.client.core.Effect
import com.navree.client.core.Engine
import com.navree.client.platform.AndroidEffects
import com.navree.client.platform.Origins
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
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.atomic.AtomicBoolean

/**
 * What a page of the app stores in the web view, and what a page shown after
 * the person signs out finds of it. One page stores a cookie, a value in local
 * storage, one in IndexedDB and one in Cache Storage, and registers a service
 * worker; the session ends through the client's own sign-out (the core's
 * `forget_session` effect, carried out by [AndroidEffects]); then a page of
 * the same origin in a new web view says what it finds. The control is the
 * same without the sign-out: the second page finds all five, so the page that
 * looks can see them.
 */
@RunWith(AndroidJUnit4::class)
class WebSignOutTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val app = MockWebServer()

    /** What the pages told the app's server, in order. */
    private val said = CopyOnWriteArrayList<String>()

    @After
    fun stop() {
        app.close()
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

    private fun serve() {
        app.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse = when (request.url.encodedPath) {
                "/says" -> {
                    said += request.url.queryParameter("m") ?: ""
                    MockResponse.Builder().code(204).build()
                }
                "/" -> page(stores)
                "/look" -> page(looks)
                "/sw.js" -> MockResponse.Builder().code(200).addHeader("Content-Type", "text/javascript").body("self.addEventListener('fetch', function(){});").build()
                else -> MockResponse.Builder().code(204).build()
            }
        }
        app.start()
        base = app.url("/").toString().trimEnd('/')
    }

    private var shown by mutableStateOf<String?>(null)

    /** One web view at a time, made new for each address shown: as a web page of the app is. */
    private fun show(path: String) {
        compose.runOnUiThread { shown = "$base$path" }
    }

    private fun waitFor(seconds: Int, what: () -> Boolean): Boolean {
        val end = System.currentTimeMillis() + seconds * 1_000L
        while (System.currentTimeMillis() < end) {
            if (what()) return true
            Thread.sleep(250)
        }
        return what()
    }

    private fun start() {
        serve()
        // The service worker's script comes from the app's own origin; nothing in this test is about the guard on it.
        compose.runOnUiThread {
            ServiceWorkerController.getInstance().setServiceWorkerClient(object : ServiceWorkerClient() {
                override fun shouldInterceptRequest(request: WebResourceRequest): WebResourceResponse? = null
            })
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
                                webViewClient = WebViewClient()
                                loadUrl(address)
                            }
                        },
                        onRelease = { it.destroy() },
                    )
                }
            }
        }
        show("/")
        assertTrue("the first page stored all five: $said", waitFor(30) { said.isNotEmpty() })
        assertEquals("the first page stored all five", listOf("stored"), said.toList())
        // The page that stored them is left, as a person leaves it before signing out.
        compose.runOnUiThread { shown = null }
        compose.waitForIdle()
    }

    /** What the page shown next finds, read from what it said. */
    private fun looked(file: String, what: String): JsonObject {
        val before = said.size
        show("/look")
        assertTrue("the second page said what it found: $said", waitFor(30) { said.size > before })
        val found = said.last()
        Shots.put(file, "$what\nthe system's web view deletes every kind of site data: ${WebViewFeature.isFeatureSupported(WebViewFeature.DELETE_BROWSING_DATA)}\nthe second page found: $found\n")
        android.util.Log.i("WebSignOutTest", "$file: $found")
        assertTrue("the second page could look: $found", found.startsWith("{"))
        return Json.parseToJsonElement(found) as JsonObject
    }

    @Test
    fun aPageShownAfterAnotherFindsWhatItStoredWhenNobodySignedOut() {
        start()
        val found = looked("web-storage-kept.txt", "no sign-out between the two pages")
        assertTrue("the cookie: $found", found.getValue("cookie").jsonPrimitive.content.contains("city=Amritsar"))
        assertEquals("local storage: $found", 1, found.getValue("local").jsonPrimitive.int)
        assertTrue("IndexedDB: $found", found.getValue("indexeddb").jsonArray.any { it.jsonPrimitive.content == "orders" })
        assertTrue("Cache Storage: $found", found.getValue("caches").jsonArray.any { it.jsonPrimitive.content == "pages" })
        assertEquals("a service worker: $found", 1, found.getValue("workers").jsonPrimitive.int)
    }

    @Test
    fun aPageShownAfterTheSessionEndedFindsNothingAnEarlierPageStored() {
        start()
        val driver = Driver(object : Engine {
            override fun send(event: ByteArray): ByteArray = "{}".encodeToByteArray()

            override fun view(query: ByteArray): ByteArray = "{}".encodeToByteArray()
        })
        lateinit var effects: AndroidEffects
        compose.runOnUiThread {
            effects = AndroidEffects(compose.activity, driver, "$base/".toHttpUrl(), null, null, Origins(base, loopbackHttp = true), loopbackHttp = true)
            effects.run(Effect.ForgetSession)
        }
        // Whatever the store does after the end of the session runs only once the wipe is done.
        val wiped = AtomicBoolean(false)
        effects.state.always { wiped.set(true) }
        assertTrue("the end of the session finished", waitFor(30) { wiped.get() })
        val found = looked("web-storage-after-sign-out.txt", "the session ended between the two pages")
        assertEquals("no cookie: $found", "", found.getValue("cookie").jsonPrimitive.content)
        assertEquals("nothing in local storage: $found", 0, found.getValue("local").jsonPrimitive.int)
        assertEquals("no IndexedDB database: $found", 0, found.getValue("indexeddb").jsonArray.size)
        assertEquals("nothing in Cache Storage: $found", 0, found.getValue("caches").jsonArray.size)
        assertEquals("no service worker: $found", 0, found.getValue("workers").jsonPrimitive.int)
        compose.runOnUiThread { effects.close() }
    }
}
