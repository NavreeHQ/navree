package com.navree.client

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeUp
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.navree.client.core.Content
import com.navree.client.core.Events
import com.navree.client.core.Stage
import com.navree.client.ui.NavreeApp
import mockwebserver3.MockWebServer
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The protocol's three health screens (today, a night's sleep, fitness) in a
 * health app's shell, drawn by the client on a real device in the look the
 * job set, each photographed from its top to its end, with the day pager's
 * date picker and a chart's hint open.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(AndroidJUnit4::class)
class HealthScreensTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val server = MockWebServer()

    @After
    fun stop() {
        server.close()
    }

    private fun start(): NavreeSession {
        server.dispatcher = Recorded(
            "",
            routes = mapOf(
                "/_navree/native/shell" to "health/shell",
                "/" to "screen-today",
                "/sleep" to "screen-sleep",
                "/fitness" to "screen-fitness",
                "/designed" to "screen-designed-health",
                "/shelves" to "screen-designed-store",
            ),
        )
        server.start()
        val origin = server.url("/").toString().trimEnd('/')
        val config = NavreeConfig(origin = origin, name = "Health", seed = null, loopbackHttp = true, openAnyLink = true)
        lateinit var made: NavreeSession
        compose.runOnUiThread {
            made = NavreeSession(compose.activity.application, config, origin.toHttpUrl())
            made.effects.activity = compose.activity
        }
        compose.setContent { NavreeApp(made.driver, config, image = made::image) }
        waitFor("the app is ready", 60_000) { made.driver.nav.stage == Stage.Ready }
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

    private fun front(session: NavreeSession, title: String) {
        waitFor("the screen \"$title\"") {
            val front = session.driver.nav.front?.let { session.driver.screens[it] }
            (front?.content as? Content.Native)?.screen?.title == title && !front.loading
        }
        // The charts grow into place; the shot is of where they settle.
        Thread.sleep(1200)
        compose.waitForIdle()
    }

    private fun send(session: NavreeSession, event: kotlinx.serialization.json.JsonObject) {
        compose.runOnUiThread { session.driver.send(event) }
    }

    /** The screen from its top down, a shot a window's height apart. */
    private fun scrolled(name: String, shots: Int) {
        Shots.take("$name-1")
        for (index in 2..shots) {
            compose.onRoot().performTouchInput { swipeUp(startY = bottom * 0.85f, endY = bottom * 0.2f, durationMillis = 600) }
            Thread.sleep(900)
            compose.waitForIdle()
            Shots.take("$name-$index")
        }
    }

    @Test
    fun theHealthScreens() {
        val app = start()
        front(app, "Today")
        scrolled("health-today", 4)

        send(app, Events.openLink("${server.url("/sleep")}"))
        front(app, "Sleep")
        scrolled("health-sleep", 5)

        send(app, Events.openLink("${server.url("/fitness")}"))
        front(app, "Fitness")
        scrolled("health-fitness", 7)

        // The same day written for native clients: cards, a pager of the screen's own, rooms.
        send(app, Events.openLink("${server.url("/designed")}"))
        front(app, "Today")
        scrolled("designed-health", 4)

        send(app, Events.openLink("${server.url("/shelves")}"))
        front(app, "Shelves")
        scrolled("designed-shelves", 4)
    }

    @Test
    fun thePickerAndAHint() {
        val app = start()
        front(app, "Today")
        compose.onNodeWithContentDescription("Today. Choose a day").performClick()
        Thread.sleep(800)
        compose.waitForIdle()
        Shots.take("health-today-picker")
        Shots.shell("input keyevent 4")
        Thread.sleep(600)
        compose.waitForIdle()
        compose.onNodeWithContentDescription("About the health score").performClick()
        Thread.sleep(800)
        compose.waitForIdle()
        Shots.take("health-today-hint")
    }
}
