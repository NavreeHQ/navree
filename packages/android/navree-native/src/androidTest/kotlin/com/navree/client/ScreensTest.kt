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
    private fun start(folder: String, routes: Map<String, String> = emptyMap(), read: ((String) -> String?)? = null): NavreeSession {
        server.dispatcher = Recorded(folder, routes, read)
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
        try {
            waitFor("the screen \"$title\"") {
                val front = session.driver.nav.front?.let { session.driver.screens[it] }
                (front?.content as? Content.Native)?.screen?.title == title && !front.loading
            }
        } catch (e: AssertionError) {
            val front = session.driver.nav.front?.let { session.driver.screens[it] }
            throw AssertionError("${e.message}; in front: ${front?.content?.let { it::class.simpleName }} titled ${(front?.content as? Content.Native)?.screen?.title}, loading ${front?.loading}, stage ${session.driver.nav.stage}", e)
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
     * The protocol's own form example without the two kinds of field this
     * client does not draw (`input:colour` and `input:file` are in its list of
     * what is shown whole in the web view, so a screen that holds one is a web
     * page): what is left is a native form.
     */
    private fun formWithoutWhatIsShownInTheWebView(): String {
        val text = androidx.test.platform.app.InstrumentationRegistry.getInstrumentation().context.assets.open("screen-form.json").use { it.readBytes().decodeToString() }
        fun clean(node: Any?) {
            when (node) {
                is org.json.JSONObject -> node.keys().asSequence().toList().forEach { clean(node.get(it)) }
                is org.json.JSONArray -> {
                    for (at in node.length() - 1 downTo 0) {
                        val part = node.get(at)
                        val kind = (part as? org.json.JSONObject)?.optJSONObject("input")?.optString("t")
                        if (kind == "colour" || kind == "file") node.remove(at) else clean(part)
                    }
                }
            }
        }
        val root = org.json.JSONObject(text)
        clean(root)
        return root.toString()
    }

    /**
     * A hardware keyboard's Tab through the protocol's own form example, which
     * has fields that are picked, not typed (a choice, a date): each is one
     * stop, the layer over the field, with its name, and the typed field under
     * it is none.
     */
    @Test
    fun aTabWalkThroughAFormStopsOnceAtEachPickedField() {
        val form = formWithoutWhatIsShownInTheWebView()
        val app = start("", routes = mapOf("/_navree/native/shell" to "shell", "/" to "screen-dashboard", "/customers/new" to "screen-form"), read = { file -> if (file == "screen-form.json") form else null })
        front(app, "Today")
        send(app, Events.openLink("${server.url("/customers/new")}"))
        front(app, "New customer")
        val stops = ArrayList<String>()
        val trace = ArrayList<String>()
        repeat(45) { press ->
            compose.onRoot().performKeyInput { pressKey(Key.Tab) }
            compose.waitForIdle()
            val focused = compose.onAllNodes(isFocused()).fetchSemanticsNodes()
            assertTrue("press ${press + 1} of Tab landed on nothing that can be seen or said; the stops so far: $stops", focused.size == 1)
            val config = focused.single().config
            val where = "#${focused.single().id}@${(focused.single().boundsInRoot.top / focused.single().layoutInfo.density.density).toInt()}"
            val named = listOfNotNull(
                config.getOrNull(SemanticsProperties.ContentDescription)?.joinToString(", "),
                config.getOrNull(SemanticsProperties.Text)?.joinToString(", ") { it.text },
                config.getOrNull(SemanticsProperties.EditableText)?.text,
            ).filter { it.isNotEmpty() }.joinToString(" | ")
            assertTrue("press ${press + 1}: the stop has no name; the stops so far: $stops", named.isNotEmpty())
            stops += named
            trace += named.take(30) + where
        }
        stops.zipWithNext().forEach { (before, after) -> assertTrue("the stop \"$after\" came twice running; the stops were $trace", before != after) }
        // Each picked field is one stop of a loop, not two: its label starts the name of one stop, in the first loop.
        for (label in listOf("City", "Tier", "Customer since")) {
            val found = stops.count { it.startsWith(label) }
            assertTrue("$label is a stop at most once in each loop of the form: $found in ${stops.size} presses; the stops were $trace", found in 1..2)
        }
    }
}
