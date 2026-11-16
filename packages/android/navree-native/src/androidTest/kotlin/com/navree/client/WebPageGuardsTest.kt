package com.navree.client

import android.webkit.WebView
import androidx.activity.ComponentActivity
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.navree.client.core.Driver
import com.navree.client.core.Engine
import com.navree.client.design.WindowInfo
import com.navree.client.platform.Origins
import com.navree.client.platform.guardDocuments
import com.navree.client.ui.LocalUi
import com.navree.client.ui.LocalWebGuards
import com.navree.client.ui.Ui
import com.navree.client.ui.WebGuards
import com.navree.client.ui.WebPage
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
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

/**
 * The client's own web page, as the app composes it, with each of its two
 * guards answered as missing: the rules for a service worker's requests, and
 * the script at the start of every document. While either is missing the page
 * shows its failed screen with "Try again", and the app's server is never
 * asked for the page; once the guard is there, "Try again" loads it. The same
 * holds while the web view still holds what the pages of a session that ended
 * stored. The real decision runs: only the answers are the test's.
 */
@RunWith(AndroidJUnit4::class)
class WebPageGuardsTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val app = MockWebServer()
    private val asked = CopyOnWriteArrayList<String>()

    @After
    fun stop() {
        app.close()
    }

    private val workers = AtomicBoolean(true)
    private val documents = AtomicBoolean(true)
    private val clean = AtomicBoolean(true)

    /** How many times the page asked for each answer. */
    private val workersAsked = AtomicInteger()
    private val documentsAsked = AtomicInteger()
    private val cleanAsked = AtomicInteger()

    private fun show() {
        app.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                asked += request.method + " " + request.url.encodedPath
                return MockResponse.Builder().code(200).addHeader("Content-Type", "text/html; charset=utf-8")
                    .body("<!doctype html><html><head><meta charset=\"utf-8\"><title>Rasid</title></head><body><p>Sangrur</p></body></html>").build()
            }
        }
        app.start()
        val base = app.url("/").toString().trimEnd('/')
        val origins = Origins(base, loopbackHttp = true)
        val driver = Driver(object : Engine {
            override fun send(event: ByteArray): ByteArray = "{}".encodeToByteArray()

            override fun view(query: ByteArray): ByteArray = "{}".encodeToByteArray()
        })
        val ui = Ui(driver, WindowInfo(400, 800), image = { null }, snackbars = SnackbarHostState(), origins = origins)
        val guards = WebGuards(
            workers = {
                workersAsked.incrementAndGet()
                workers.get()
            },
            documents = { view: WebView, them: Origins ->
                documentsAsked.incrementAndGet()
                // The real script is added where the answer is yes, so a page that loads is guarded as in the app.
                documents.get() && guardDocuments(view, them)
            },
            clean = {
                cleanAsked.incrementAndGet()
                clean.get()
            },
        )
        compose.setContent {
            MaterialTheme {
                CompositionLocalProvider(LocalUi provides ui, LocalWebGuards provides guards) {
                    WebPage(url = "$base/", serial = 1, onLink = {}, onTitle = {})
                }
            }
        }
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

    /** The failed screen is shown, and in the time a page takes to load the app's server is asked nothing. */
    private fun refused(what: String) {
        compose.waitUntil(10_000) { compose.onAllNodes(androidx.compose.ui.test.hasText("Try again")).fetchSemanticsNodes().isNotEmpty() }
        // As long as a page takes to be asked for once the guard is there, with the composition drawing all the while.
        assertTrue("the page was asked for with $what: $asked", !waitFor(5) { asked.isNotEmpty() })
        Shots.put("web-page-$what.txt", "a web page with $what\nthe app's server was asked: $asked\nworkers asked ${workersAsked.get()}, documents asked ${documentsAsked.get()}\n")
        assertEquals("the app's server was asked nothing", emptyList<String>(), asked.toList())
    }

    /** "Try again" with the guard there: the page is asked for, so the refusal above was the guard's. */
    private fun loadsOnceTheGuardIsThere() {
        compose.onNodeWithText("Try again").performClick()
        assertTrue("the page was asked for once the guard was there: $asked", waitFor(15) { asked.contains("GET /") })
    }

    @Test
    fun aWebPageIsNotLoadedWhileTheRulesForAServiceWorkersRequestsAreNotInForce() {
        workers.set(false)
        show()
        refused("no rules for service workers")
        assertTrue("the rules were asked for", workersAsked.get() >= 1)
        workers.set(true)
        loadsOnceTheGuardIsThere()
    }

    @Test
    fun aWebPageIsNotLoadedWhenTheScriptAtTheStartOfEveryDocumentCouldNotBeAdded() {
        documents.set(false)
        show()
        refused("no script at the start of every document")
        assertTrue("the web view was made and the script asked for: ${documentsAsked.get()}", documentsAsked.get() >= 1)
        documents.set(true)
        loadsOnceTheGuardIsThere()
    }

    @Test
    fun aWebPageIsNotLoadedWhileTheWebViewStillHoldsWhatAnEndedSessionsPagesStored() {
        clean.set(false)
        show()
        refused("something of an ended session left in the web view")
        assertTrue("the page asked whether the web view is clean", cleanAsked.get() >= 1)
        clean.set(true)
        loadsOnceTheGuardIsThere()
    }
}
