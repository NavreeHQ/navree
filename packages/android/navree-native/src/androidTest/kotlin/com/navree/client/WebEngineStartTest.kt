package com.navree.client

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.webkit.CookieManager
import android.webkit.ServiceWorkerController
import android.webkit.WebView
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.navree.client.platform.Origins
import com.navree.client.platform.WorkerGuard
import com.navree.client.platform.WebViewCookies
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * What the web view's engine costs the first time something asks for it, and
 * what each thing that needs it costs once it is running. Each test is run in
 * a process of its own when it is to say anything about the first call (the job
 * starts one instrumentation for each, with `-e cold 1`); in a process that has
 * run other tests the engine is running already and the numbers say so.
 */
@RunWith(AndroidJUnit4::class)
class WebEngineStartTest {
    private val lines = mutableListOf<String>()
    private val cold get() = InstrumentationRegistry.getArguments().getString("cold") == "1"

    private fun <T> timed(what: String, call: () -> T): T {
        val start = SystemClock.elapsedRealtimeNanos()
        val result = call()
        lines += "$what: ${(SystemClock.elapsedRealtimeNanos() - start) / 1_000} us"
        return result
    }

    /** Runs [call] on a thread of its own, as a native request's cookie jar does, and waits for it. */
    private fun <T> onARequestThread(call: () -> T): T {
        var result: Result<T>? = null
        val done = CountDownLatch(1)
        Thread {
            result = runCatching(call)
            done.countDown()
        }.start()
        assertTrue("the call came back", done.await(60, TimeUnit.SECONDS))
        return result!!.getOrThrow()
    }

    private fun onTheMainThread(call: () -> Unit) {
        val done = CountDownLatch(1)
        var failure: Throwable? = null
        Handler(Looper.getMainLooper()).post {
            try {
                call()
            } catch (e: Throwable) {
                failure = e
            }
            done.countDown()
        }
        assertTrue("the main thread ran it", done.await(60, TimeUnit.SECONDS))
        failure?.let { throw it }
    }

    private fun say(order: String) {
        val text = "the engine, first asked for by $order (a ${if (cold) "cold" else "warm"} process)\n" + lines.joinToString("\n") + "\n"
        android.util.Log.i("WebEngineStart", text.replace("\n", " | "))
        Shots.put("web-engine-$order-${if (cold) "cold" else "warm"}.txt", text)
    }

    @Test
    fun theFirstNativeRequestStartsTheEngineThroughTheCookieJar() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val url = "https://shop.example/".toHttpUrl()
        // The first request of a start reads the cookies for its address, on a thread of the request's own.
        val first = onARequestThread { timed("the first read of cookies, as the first request makes it") { WebViewCookies().loadForRequest(url) } }
        onARequestThread { timed("the next read of cookies") { WebViewCookies().loadForRequest(url) } }
        onTheMainThread { timed("the guard on service workers, once the engine is running") { WorkerGuard.arm(Origins("https://shop.example")); WorkerGuard.ensure() } }
        onTheMainThread { timed("a web view made, once the engine is running") { WebView(context).destroy() } }
        say("the cookie jar")
        assertTrue("the read came back: $first", first.isEmpty())
    }

    @Test
    fun theGuardOnServiceWorkersStartsTheEngineWhenItIsAskedFirst() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val url = "https://shop.example/".toHttpUrl()
        onTheMainThread { timed("the guard on service workers, first asked for") { WorkerGuard.arm(Origins("https://shop.example")); WorkerGuard.ensure() } }
        onARequestThread { timed("the first read of cookies, once the engine is running") { WebViewCookies().loadForRequest(url) } }
        onTheMainThread { timed("the cookie manager's instance, once the engine is running") { CookieManager.getInstance() } }
        onTheMainThread { timed("the service worker controller's instance, once the engine is running") { ServiceWorkerController.getInstance() } }
        onTheMainThread { timed("a web view made, once the engine is running") { WebView(context).destroy() } }
        say("the guard on service workers")
    }
}
