package com.navree.client

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.navree.client.core.Content
import com.navree.client.core.Events
import com.navree.client.core.Stage
import com.navree.client.ui.NavreeApp
import mockwebserver3.MockWebServer
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The client against answers recorded from a server, on a real device: the
 * runtime core's library, the client's effects and its drawing, each screen
 * photographed in the look the job set (light, dark, the largest text, right
 * to left) on the device it runs on (a phone, a tablet).
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(AndroidJUnit4::class)
class ScreensTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val server = MockWebServer()

    @After
    fun stop() {
        server.close()
    }

    /** Starts the app against the recorded answers in `folder`, and waits for its first screen. */
    private fun start(folder: String, routes: Map<String, String> = emptyMap()): NavreeSession {
        server.dispatcher = Recorded(folder, routes)
        server.start()
        val origin = server.url("/").toString().trimEnd('/')
        val config = NavreeConfig(origin = origin, name = "Charkha Supply Co.", seed = null, loopbackHttp = true, openAnyLink = true)
        lateinit var made: NavreeSession
        compose.runOnUiThread {
            made = NavreeSession(compose.activity.application, config, origin.toHttpUrl())
            made.effects.activity = compose.activity
        }
        compose.setContent { NavreeApp(made.driver, config, image = made::image) }
        waitFor("the app is ready") { made.driver.nav.stage == Stage.Ready }
        return made
    }

    private fun waitFor(what: String, timeout: Long = 20_000, check: () -> Boolean) {
        try {
            compose.waitUntil(timeout) { check() }
        } catch (e: Throwable) {
            throw AssertionError("waited for $what", e)
        }
        compose.waitForIdle()
    }

    /** Waits for the screen in front to show a tree titled `title`, then for drawing to settle. */
    private fun front(session: NavreeSession, title: String) {
        waitFor("the screen \"$title\"") {
            val front = session.driver.nav.front?.let { session.driver.screens[it] }
            (front?.content as? Content.Native)?.screen?.title == title && !front.loading
        }
        Thread.sleep(600)
        compose.waitForIdle()
    }

    private fun send(session: NavreeSession, event: kotlinx.serialization.json.JsonObject) {
        compose.runOnUiThread { session.driver.send(event) }
    }

    @Test
    fun theStoreDemosRecordedScreens() {
        val app = start("store-demo")
        // The dashboard was recorded as a page for the web view, inside the app's own chrome.
        waitFor("the dashboard") { app.driver.screens.values.any { it.content is Content.Web } }
        Thread.sleep(2500)
        Shots.take("store-dashboard-web")

        send(app, Events.select(1))
        front(app, "Orders")
        Shots.take("store-orders")

        send(app, Events.openLink("${server.url("/orders/1")}"))
        front(app, "Order CS-20481")
        Shots.take("store-order")

        send(app, Events.select(2))
        front(app, "Inventory")
        Shots.take("store-inventory")
        assertEquals(Stage.Ready, app.driver.nav.stage)
    }

    @Test
    fun theProtocolsExampleScreens() {
        val app = start(
            "",
            routes = mapOf(
                "/_navree/native/shell" to "shell",
                "/" to "screen-dashboard",
                "/orders" to "screen-list",
                "/orders/1042" to "screen-detail",
                "/orders/1041" to "screen-detail",
                "/inventory" to "screen-with-toast",
                "/inbox" to "screen-inbox",
            ),
        )
        front(app, "Today")
        Shots.take("examples-dashboard")

        send(app, Events.select(1))
        front(app, "Orders")
        Shots.take("examples-orders")

        send(app, Events.openLink("${server.url("/orders/1042")}"))
        front(app, "Order CH-1042")
        Shots.take("examples-order")

        send(app, Events.openLink("${server.url("/inbox")}"))
        front(app, "Inbox")
        Shots.take("examples-inbox")

        send(app, Events.select(2))
        front(app, "Inventory")
        Shots.take("examples-inventory-empty")
    }
}
