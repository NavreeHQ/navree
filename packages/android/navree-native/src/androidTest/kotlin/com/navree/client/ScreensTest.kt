package com.navree.client

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.performKeyInput
import org.junit.Assert.assertTrue
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.hasContentDescription
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.navree.client.core.Content
import com.navree.client.core.Events
import com.navree.client.core.Stage
import mockwebserver3.MockWebServer
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Before
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

    @Before
    fun readingListsFollowThePictures() {
        Shots.rule = compose
    }

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

    /** Waits for the screen in front to show a tree titled `title`, then for drawing to settle. */
    private fun front(session: NavreeSession, title: String) {
        waitFor("the screen \"$title\"") {
            val front = session.driver.nav.front?.let { session.driver.screens[it] }
            (front?.content as? Content.Native)?.screen?.title == title && !front.loading
        }
        Thread.sleep(600)
        compose.waitForIdle()
    }

    /** What the screen in front is, as a tree, for the file beside each picture. */
    private fun treeOf(session: NavreeSession): String? =
        session.driver.nav.front?.let { session.driver.screens[it] }?.screen?.let { com.navree.protocol.Navree.json.encodeToString(com.navree.protocol.Navree.Screen.serializer(), it) }

    private fun send(session: NavreeSession, event: kotlinx.serialization.json.JsonObject) {
        compose.runOnUiThread { session.driver.send(event) }
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

    /**
     * The menus and a question: the account's menu, a filter's choices, the
     * question a destructive action asks, each pictured open.
     */
    @Test
    fun theMenusAndAQuestion() {
        val app = start(
            "",
            routes = mapOf(
                "/_navree/native/shell" to "shell",
                "/" to "screen-dashboard",
                "/orders" to "screen-list",
                "/orders/1042" to "screen-detail",
                "/orders/1041" to "screen-detail",
            ),
        )
        front(app, "Today")
        compose.onAllNodes(hasContentDescription("Account", substring = true)).onFirst().performClick()
        Thread.sleep(700)
        compose.waitForIdle()
        Shots.take("menu-account")
        Shots.requireWindowsFollowTheScreen("the account's menu")
        Shots.shell("input keyevent 4")
        Thread.sleep(500)
        compose.waitForIdle()
        send(app, Events.select(1))
        front(app, "Orders")
        compose.onNodeWithText("Newest first").performClick()
        Thread.sleep(700)
        compose.waitForIdle()
        Shots.take("menu-sort")
        Shots.requireWindowsFollowTheScreen("a sort's menu")
        Shots.shell("input keyevent 4")
        Thread.sleep(500)
        compose.waitForIdle()
        send(app, Events.openLink("${server.url("/orders/1042")}"))
        front(app, "Order CH-1042")
        compose.onNodeWithText("Cancel order").performClick()
        Thread.sleep(700)
        compose.waitForIdle()
        Shots.take("question-cancel-order")
        Shots.requireWindowsFollowTheScreen("the question")
    }

    /**
     * A hardware keyboard's Tab through a form stops only where something can be
     * seen and said: every stop is a control with a name and a bound a finger
     * could hit, and the field that is picked, not typed, is one stop.
     */
    @Test
    fun aTabWalkThroughAFormStopsOnlyWhereSomethingCanBeSeenAndSaid() {
        val app = start("", routes = mapOf("/_navree/native/shell" to "shell", "/" to "screen-form"))
        front(app, "New customer")
        val stops = ArrayList<String>()
        repeat(40) { press ->
            compose.onRoot().performKeyInput { pressKey(Key.Tab) }
            compose.waitForIdle()
            val focused = compose.onAllNodes(isFocused()).fetchSemanticsNodes()
            assertTrue("press ${press + 1} of Tab landed on nothing that can be seen or said; the stops so far: $stops", focused.size == 1)
            val node = focused.single()
            val density = node.layoutInfo.density.density
            val config = node.config
            val named = listOfNotNull(
                config.getOrNull(SemanticsProperties.ContentDescription)?.joinToString(", "),
                config.getOrNull(SemanticsProperties.Text)?.joinToString(", ") { it.text },
                config.getOrNull(SemanticsProperties.EditableText)?.text,
            ).filter { it.isNotEmpty() }.joinToString(" | ")
            assertTrue("press ${press + 1}: the stop has no name; the stops so far: $stops", named.isNotEmpty())
            assertTrue("press ${press + 1}: \"$named\" is ${node.size.width / density}x${node.size.height / density} dp, too small to see", node.size.width / density >= 8f && node.size.height / density >= 8f)
            stops += named.take(40)
        }
        // A field picked from a list (a city, a tier) is one stop: no stop has the same name as the one before it.
        stops.zipWithNext().forEach { (before, after) -> assertTrue("the stop \"$after\" came twice running", before != after) }
    }
}
