package com.navree.client

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollAction
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeRight
import androidx.compose.ui.test.swipeUp
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.navree.client.core.Content
import com.navree.client.core.Events
import com.navree.client.core.Stage
import mockwebserver3.MockWebServer
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.After
import org.junit.Before
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

    @Before
    fun readingListsFollowThePictures() {
        Shots.rule = compose
    }

    private val server = MockWebServer()
    private lateinit var recorded: Recorded

    @After
    fun stop() {
        server.close()
    }

    /** The app on its first screen: the plain today screen, or the designed one as the first screen of its destination. */
    private fun start(designedAtRoot: Boolean = false): NavreeSession {
        recorded = Recorded(
            "",
            routes = mapOf(
                "/_navree/native/shell" to "health/shell",
                "/" to if (designedAtRoot) "screen-designed-health" else "screen-today",
                "/sleep" to "screen-sleep",
                "/fitness" to "screen-fitness",
                "/designed" to "screen-designed-health",
                // A screen with no destination of its own: opened from a card, it is pushed on the screen it came from.
                "/heart" to "screen-designed-store",
                "/shelves" to "screen-designed-store",
            ),
        )
        server.dispatcher = recorded
        server.start()
        val origin = server.url("/").toString().trimEnd('/')
        val config = NavreeConfig(origin = origin, name = "Health", seed = null, loopbackHttp = true, openAnyLink = true)
        lateinit var made: NavreeSession
        compose.runOnUiThread {
            made = NavreeSession(compose.activity.application, config, origin.toHttpUrl())
            made.effects.activity = compose.activity
        }
        Shots.tree = { treeOf(made) }
        compose.setContent { Shots.Looked { Host(made) } }
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
        Thread.sleep(1800)
        compose.waitForIdle()
    }

    /** What the screen in front is, as a tree, for the file beside each picture. */
    private fun treeOf(session: NavreeSession): String? =
        session.driver.nav.front?.let { session.driver.screens[it] }?.screen?.let { com.navree.protocol.Navree.json.encodeToString(com.navree.protocol.Navree.Screen.serializer(), it) }

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

        send(app, Events.openLink("${server.url("/shelves")}"))
        front(app, "Shelves")
        scrolled("designed-shelves", 4)
    }

    /** The same day written for native clients: cards, a pager of the screen's own, rooms; the first screen of Today. */
    @Test
    fun theDesignedHealthScreen() {
        val app = start(designedAtRoot = true)
        front(app, "Today")
        scrolled("designed-health", 4)
    }

    /** A card that zooms into the screen it opens, and the way back into it. */
    @Test
    fun aCardOpensItsScreen() {
        val app = start(designedAtRoot = true)
        front(app, "Today")
        // At the largest text the card is below the first screen: scroll to it first.
        compose.onAllNodes(hasScrollAction()).onFirst().performScrollToNode(hasContentDescription("Heart, 20% of score"))
        compose.onNodeWithContentDescription("Heart, 20% of score").performClick()
        front(app, "Shelves")
        Shots.take("designed-health-opened")
        send(app, Events.back)
        front(app, "Today")
        Shots.take("designed-health-back")
    }

    /**
     * The zoom from a card into the screen it opens, caught while it moves: the
     * test clock is held and let on a few frames at a time, a picture taken
     * between. A device that does not let the clock be held leaves no picture.
     */
    @Test
    fun aCardZoomIsDrawnWhileItMoves() {
        val app = start(designedAtRoot = true)
        front(app, "Today")
        compose.onAllNodes(hasScrollAction()).onFirst().performScrollToNode(hasContentDescription("Heart, 20% of score"))
        runCatching {
            compose.mainClock.autoAdvance = false
            compose.onNodeWithContentDescription("Heart, 20% of score").performClick()
            val end = System.currentTimeMillis() + 15_000
            while (System.currentTimeMillis() < end) {
                val front = app.driver.nav.front?.let { app.driver.screens[it] }
                if ((front?.content as? Content.Native)?.screen?.title == "Shelves") break
                compose.mainClock.advanceTimeByFrame()
                Thread.sleep(16)
            }
            for (step in 1..4) {
                compose.mainClock.advanceTimeBy(70)
                Shots.take("designed-health-zoom-$step")
            }
        }.onFailure { android.util.Log.w("NavreeShots", "the zoom was not caught: $it") }
        compose.mainClock.autoAdvance = true
        compose.waitForIdle()
    }

    /** A screen of days turns to the day before or after when it is swiped along, the way the page moves with the finger. */
    @Test
    fun aSwipeTurnsTheDay() {
        val app = start()
        front(app, "Today")
        send(app, Events.openLink("${server.url("/sleep")}"))
        front(app, "Sleep")
        compose.onRoot().performTouchInput { swipeRight(startX = left + 40f, endX = right - 40f, durationMillis = 300) }
        val wanted = if (Shots.look == "rtl") "GET /sleep?date=2026-11-13" else "GET /sleep?date=2026-11-11"
        waitFor("the day $wanted") { recorded.asked.contains(wanted) }
        Thread.sleep(1500)
        compose.waitForIdle()
        Shots.take("health-sleep-swiped")
    }

    /**
     * A chart read out where it is touched: a finger held on the first chart of
     * the fitness screen's week, a picture taken while it stays there.
     */
    @Test
    fun aChartReadsOutWhereItIsTouched() {
        val app = start()
        front(app, "Today")
        send(app, Events.openLink("${server.url("/fitness")}"))
        front(app, "Fitness")
        runCatching {
            val chart = hasContentDescription("Active minutes by intensity, last 7 days:", substring = true) and hasScrollAction().not()
            compose.onAllNodes(hasScrollAction()).onFirst().performScrollToNode(chart)
            compose.onAllNodes(chart).onFirst().performTouchInput { down(Offset(width * 0.6f, height * 0.5f)) }
            Thread.sleep(500)
            compose.waitForIdle()
            Shots.take("health-fitness-readout")
            compose.onAllNodes(chart).onFirst().performTouchInput { up() }
        }.onFailure { android.util.Log.w("NavreeShots", "the chart was not touched: $it") }
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
