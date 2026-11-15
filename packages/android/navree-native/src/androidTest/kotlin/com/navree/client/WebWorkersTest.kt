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
import com.navree.client.platform.guardWorkers
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import org.junit.After
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.util.Collections

/**
 * What a page of the app starts in the web view beside its own loads: a
 * service worker it registers, and a WebSocket it opens. The other origin is
 * another server on this device. A service worker's own requests go through
 * the web rules as a page's do, so a worker never reaches another origin. A
 * socket is not a request the web view's client is asked about: the test says
 * whether this system's web view lets one through.
 */
@RunWith(AndroidJUnit4::class)
class WebWorkersTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val app = MockWebServer()
    private val other = MockWebServer()
    private val appAsked = Collections.synchronizedList(ArrayList<String>())
    private val otherAsked = Collections.synchronizedList(ArrayList<String>())

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

    private fun openedBy(page: String, origins: Origins, base: String): WebView {
        var web: WebView? = null
        compose.setContent {
            AndroidView(
                modifier = Modifier.fillMaxSize(),
                factory = { context ->
                    WebView(context).apply {
                        settings.javaScriptEnabled = true
                        webViewClient = OriginWebClient(origins, onLink = {})
                        guardDocuments(this, origins)
                        web = this
                    }
                },
            )
        }
        compose.waitUntil(10_000) { web != null }
        compose.runOnUiThread { web!!.loadUrl("$base$page") }
        compose.waitUntil(15_000) { appAsked.any { it.endsWith(" $page") } }
        return web!!
    }

    @Test
    fun aServiceWorkerNeverReachesAnotherOrigin() {
        app.start()
        other.start()
        val base = app.url("/").toString().trimEnd('/')
        val elsewhere = other.url("/").toString().trimEnd('/')
        val origins = Origins(base, loopbackHttp = true)
        val guarded = guardWorkers(origins)
        android.util.Log.i("WebWorkersTest", "the worker rules are on: $guarded")
        other.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                otherAsked += request.method + " " + request.url.encodedPath
                return MockResponse.Builder().code(200).addHeader("Access-Control-Allow-Origin", "*").body("elsewhere").build()
            }
        }
        // The worker asks the other origin when it installs, when it starts, and for what the page fetches.
        val worker = "self.addEventListener('install', function(e){ self.skipWaiting(); e.waitUntil(fetch('$elsewhere/from-install', {mode: 'no-cors'}).catch(function(){})) });" +
            "self.addEventListener('activate', function(e){ e.waitUntil(self.clients.claim().then(function(){ return fetch('$elsewhere/from-activate', {mode: 'no-cors'}).catch(function(){}) })) });" +
            "self.addEventListener('fetch', function(e){ if (new URL(e.request.url).pathname === '/through-the-worker') e.respondWith(fetch('$elsewhere/from-fetch', {mode: 'no-cors'}).catch(function(){ return new Response('blocked') })) });"
        app.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                val path = request.url.encodedPath
                appAsked += request.method + " " + path
                return when (path) {
                    "/" -> page("<p>ours</p><script>navigator.serviceWorker.register('/sw.js').then(function(){ return navigator.serviceWorker.ready }).then(function(){ return fetch('/through-the-worker') }).then(function(r){ new Image().src = '/registered?' + r.status }).catch(function(e){ new Image().src = '/not-registered?' + encodeURIComponent(String(e)) })</script>")
                    "/sw.js" -> MockResponse.Builder().code(200).addHeader("Content-Type", "text/javascript").body(worker).build()
                    else -> MockResponse.Builder().code(204).build()
                }
            }
        }
        openedBy("/", origins, base)
        // The worker installs, starts, and is asked for a path of the page.
        Thread.sleep(8_000)
        compose.waitForIdle()
        val registered = appAsked.any { it.endsWith(" /sw.js") }
        android.util.Log.i("WebWorkersTest", "the worker script was asked for: $registered; the app was asked $appAsked; the other origin $otherAsked")
        Shots.put("worker-asked.txt", "registered: $registered\nguarded: $guarded\napp asked: $appAsked\nother asked: $otherAsked\n")
        assertTrue("the page registered its worker", registered)
        assertFalse("a worker reached the other origin: $otherAsked", otherAsked.isNotEmpty())
    }

    @Test
    fun whatASocketToAnotherOriginDoes() {
        app.start()
        other.start()
        val base = app.url("/").toString().trimEnd('/')
        val origins = Origins(base, loopbackHttp = true)
        val reached = Collections.synchronizedList(ArrayList<String>())
        other.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                otherAsked += request.method + " " + request.url.encodedPath + " upgrade=" + request.headers["Upgrade"]
                return MockResponse.Builder().webSocketUpgrade(object : WebSocketListener() {
                    override fun onOpen(webSocket: WebSocket, response: Response) {
                        reached += "opened"
                    }
                }).build()
            }
        }
        app.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                val path = request.url.encodedPath
                appAsked += request.method + " " + path
                return if (path == "/") {
                    page("<p>ours</p><script>try { var s = new WebSocket('ws://localhost:${other.port}/socket'); s.onopen = function(){ new Image().src = '/socket-open' }; s.onerror = function(){ new Image().src = '/socket-error' } } catch (e) { new Image().src = '/socket-refused' }</script>")
                } else {
                    MockResponse.Builder().code(204).build()
                }
            }
        }
        openedBy("/", origins, base)
        Thread.sleep(4_000)
        compose.waitForIdle()
        val said = "a page's socket to another origin: the other server was asked $otherAsked, the socket opened: $reached, the page was told: ${appAsked.filter { it.contains("socket") }}"
        android.util.Log.i("WebWorkersTest", said)
        Shots.put("socket-asked.txt", said + "\n")
    }
}
