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
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.net.ServerSocket
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.concurrent.thread

/**
 * What else can leave a page of the app that is not a request the web view's client is asked about
 * and not a WebSocket: a WebRTC connection to a STUN server, a WebTransport session, and a
 * connection the page asks the engine to open ahead of time. The other host is a UDP socket and a TCP
 * socket on this device that count what arrives. The platform has no setting that turns these off
 * (`WebSettings` and `WebSettingsCompat` on developer.android.com name none), so the README says a page
 * may reach an address of its choice this way, and each test shows it with the client's checks on.
 */
@RunWith(AndroidJUnit4::class)
class WebOtherReachTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val app = MockWebServer()
    private val udp = DatagramSocket(0, InetAddress.getByName("127.0.0.1"))
    private val tcp = ServerSocket(0, 50, InetAddress.getByName("127.0.0.1"))
    private val packets = CopyOnWriteArrayList<Int>()
    private val connections = CopyOnWriteArrayList<String>()
    private val said = CopyOnWriteArrayList<String>()

    init {
        thread(isDaemon = true) {
            val buffer = ByteArray(2048)
            while (!udp.isClosed) {
                try {
                    val packet = DatagramPacket(buffer, buffer.size)
                    udp.receive(packet)
                    packets += packet.length
                } catch (_: Exception) {
                    return@thread
                }
            }
        }
        thread(isDaemon = true) {
            while (!tcp.isClosed) {
                try {
                    val socket = tcp.accept()
                    connections += socket.remoteSocketAddress.toString()
                    socket.close()
                } catch (_: Exception) {
                    return@thread
                }
            }
        }
    }

    @After
    fun stop() {
        app.close()
        udp.close()
        tcp.close()
    }

    private fun say(what: String) = "new Image().src = '/says?m=' + encodeURIComponent('$what')"

    private fun open(body: String) {
        app.start()
        val base = app.url("/").toString().trimEnd('/')
        val origins = Origins(base, loopbackHttp = true)
        app.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse = when (request.url.encodedPath) {
                "/says" -> {
                    said += request.url.queryParameter("m") ?: ""
                    MockResponse.Builder().code(204).build()
                }
                "/" -> MockResponse.Builder()
                    .code(200)
                    .addHeader("Content-Type", "text/html; charset=utf-8")
                    .body("<!doctype html><html><head><meta charset=\"utf-8\">${body.substringBefore("<!--body-->")}</head><body>${body.substringAfter("<!--body-->", "")}</body></html>")
                    .build()
                else -> MockResponse.Builder().code(204).build()
            }
        }
        var web: WebView? = null
        var added = false
        compose.setContent {
            AndroidView(
                modifier = Modifier.fillMaxSize(),
                factory = { context ->
                    WebView(context).apply {
                        settings.javaScriptEnabled = true
                        webViewClient = OriginWebClient(origins, onLink = {})
                        added = guardDocuments(this, origins)
                        web = this
                    }
                },
            )
        }
        compose.waitUntil(10_000) { web != null }
        assertTrue("the check at the start of every document was added", added)
        compose.runOnUiThread { web!!.loadUrl("$base/") }
    }

    private fun waitFor(seconds: Int, what: () -> Boolean): Boolean {
        val end = System.currentTimeMillis() + seconds * 1_000L
        while (System.currentTimeMillis() < end) {
            if (what()) return true
            Thread.sleep(250)
        }
        return what()
    }

    private fun record(file: String, what: String) {
        val text = "$what\nthe page said: $said\nudp packets: $packets\ntcp connections: $connections\n"
        android.util.Log.i("WebOtherReachTest", "$file: $text")
        Shots.put(file, text)
    }

    @Test
    fun aPageReachesAStunServerOfItsChoiceThroughWebRtc() {
        open(
            "<!--body--><p>ours</p><script>try { var pc = new RTCPeerConnection({iceServers: [{urls: 'stun:127.0.0.1:${udp.localPort}'}]}); pc.createDataChannel('x'); " +
                "pc.createOffer().then(function(o){ return pc.setLocalDescription(o) }); ${say("webrtc-started")} } catch (e) { ${say("webrtc-refused")} }</script>",
        )
        assertTrue("the page said whether it started: $said", waitFor(20) { said.isNotEmpty() })
        assumeTrue("this system's web view has no WebRTC: $said", said.contains("webrtc-started"))
        val reached = waitFor(20) { packets.isNotEmpty() }
        record("webrtc-reach.txt", "WebRTC to a STUN address on another host, with the client's checks on")
        assertTrue("the STUN server was sent a packet: $packets", reached)
    }

    @Test
    fun aPageReachesAServerOfItsChoiceThroughWebTransport() {
        open(
            "<!--body--><p>ours</p><script>if (typeof WebTransport === 'function') { ${say("webtransport-present")}; try { new WebTransport('https://127.0.0.1:${udp.localPort}/x').closed.catch(function(){}) } catch (e) { ${say("webtransport-refused")} } } else { ${say("webtransport-absent")} }</script>",
        )
        assertTrue("the page said whether the API is there: $said", waitFor(20) { said.isNotEmpty() })
        assumeTrue("this system's web view has no WebTransport: $said", said.contains("webtransport-present"))
        val reached = waitFor(20) { packets.isNotEmpty() }
        record("webtransport-reach.txt", "WebTransport to an address on another host, with the client's checks on")
        assertTrue("the other host was sent a packet: $packets", reached)
    }

    @Test
    fun aPageOpensAConnectionToAnAddressOfItsChoiceWithAPreconnectHint() {
        open("<link rel=\"preconnect\" href=\"http://127.0.0.1:${tcp.localPort}\"><!--body--><p>ours</p>")
        val reached = waitFor(20) { connections.isNotEmpty() }
        record("preconnect-reach.txt", "a preconnect hint to an address on another host, with the client's checks on")
        assertTrue("the other host was connected to: $connections", reached)
    }
}
