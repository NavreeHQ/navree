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
     * A hardware keyboard's Tab through the protocol's own form example, whole,
     * which has fields that are picked, not typed (a choice, a date, a colour):
     * each is one stop, the layer over the field, with its name, and the typed
     * field under it is none. Its file field is one stop too while it holds no
     * file: the button that opens the picker, which names the field.
     */
    @Test
    fun aTabWalkThroughAFormStopsOnceAtEachPickedField() {
        val app = start("", routes = mapOf("/_navree/native/shell" to "shell", "/" to "screen-dashboard", "/customers/new" to "screen-form"))
        front(app, "Today")
        send(app, Events.openLink("${server.url("/customers/new")}"))
        front(app, "New customer")
        // Tab goes round: the walk goes on until the first stop comes again, and the loop it made is what is judged.
        val ids = ArrayList<Int>()
        val names = ArrayList<String>()
        val trace = ArrayList<String>()
        var closed = false
        repeat(96) { press ->
            if (closed) return@repeat
            Shots.pressTab()
            compose.waitForIdle()
            val focused = compose.onAllNodes(isFocused()).fetchSemanticsNodes()
            assertTrue("press ${press + 1} of Tab landed on nothing that can be seen or said; the stops so far: $trace", focused.size == 1)
            val node = focused.single()
            val config = node.config
            val named = listOfNotNull(
                config.getOrNull(SemanticsProperties.ContentDescription)?.joinToString(", "),
                config.getOrNull(SemanticsProperties.Text)?.joinToString(", ") { it.text },
                config.getOrNull(SemanticsProperties.EditableText)?.text,
            ).filter { it.isNotEmpty() }.joinToString(" | ")
            assertTrue("press ${press + 1}: the stop has no name; the stops so far: $trace", named.isNotEmpty())
            if (ids.isNotEmpty() && node.id == ids.first()) {
                closed = true
            } else {
                assertTrue("the stop \"$named\" is a stop twice in one loop; the stops were $trace", node.id !in ids)
                ids += node.id
                names += named
                trace += named.take(30) + "#" + node.id
            }
        }
        assertTrue("Tab went round and came back to the first stop within 96 presses; the stops were $trace", closed)
        // A picked field is one stop, the layer over it, named by its label and what it holds; a field of options (radio buttons, chips) is a stop for each option.
        for (label in listOf("City", "Customer since", "Call at", "Next visit", "Label colour")) {
            val found = names.count { it.startsWith(label) }
            assertTrue("$label is one stop of the loop: $found; the stops were $trace", found == 1)
        }
        // The file field holds nothing yet: its one stop is the button that opens the picker, and it says which field it is for.
        val papers = names.count { it.contains("Papers") }
        assertTrue("the file field Papers is one stop of the loop: $papers; the stops were $trace", papers == 1)
    }

    /**
     * A locked cover (the sign-in a session that ended was sent to) covers the
     * whole window on phones and tablets even when the route asked for a sheet
     * with chrome: no floating dialog window, no Close button, and nothing of
     * the screens or navigation underneath visible, spoken by TalkBack, or
     * reachable by Tab.
     */
    @Test
    fun aLockedCoverCoversTheWholeWindowWithoutACloseButton() {
        val shell = """
            {"navree":1,"build":"1","name":"Charkha Supply Co.","home":"/",
             "nav":{"groups":[{"label":"Store","items":[{"label":"Today","href":"/"},{"label":"Orders","href":"/orders"},{"label":"Inventory","href":"/inventory"}]}]},
             "user":{"name":"Harpreet Sandhu","email":"harpreet.sandhu@example.com","menu":[]},
             "routes":[{"match":"/sign-in","present":"sheet"}]}
        """.trimIndent()
        val home = """{"navree":1,"build":"1","vocab":0,"screen":{"title":"Today","body":{"t":"text","value":"Ludhiana warehouse"}}}"""
        val ended = """{"navree":1,"build":"1","vocab":0,"ops":[{"op":"navigate","href":"/sign-in"}]}"""
        val signIn = """
            {"navree":1,"build":"1","vocab":0,"screen":{"title":"Sign in","body":{
              "t":"form","id":"login","method":"post","action":"/sign-in","children":[
                {"t":"field","id":"login.email","name":"email","label":"Email","input":{"t":"text","kind":"email"}},
                {"t":"button","id":"login.go","label":"Continue","role":"primary","action":{"t":"submit"}}
              ]}}}
        """.trimIndent()
        server.dispatcher = object : mockwebserver3.Dispatcher() {
            override fun dispatch(request: mockwebserver3.RecordedRequest): mockwebserver3.MockResponse = when (request.url.encodedPath) {
                "/_navree/native/shell" -> mockwebserver3.MockResponse.Builder().code(200).addHeader("Content-Type", "application/vnd.navree.native+json").body(shell).build()
                "/" -> mockwebserver3.MockResponse.Builder().code(200).addHeader("Content-Type", "application/vnd.navree.native+json").body(home).build()
                "/orders" -> mockwebserver3.MockResponse.Builder().code(401).addHeader("Content-Type", "application/vnd.navree.native+json").body(ended).build()
                "/sign-in" -> mockwebserver3.MockResponse.Builder().code(200).addHeader("Content-Type", "application/vnd.navree.native+json").body(signIn).build()
                else -> mockwebserver3.MockResponse.Builder().code(404).build()
            }
        }
        server.start()
        val origin = server.url("/").toString().trimEnd('/')
        val config = NavreeConfig(origin = origin, name = "Charkha Supply Co.", seed = null, loopbackHttp = true)
        lateinit var app: NavreeSession
        compose.runOnUiThread {
            app = NavreeSession(compose.activity.application, config, origin.toHttpUrl())
            app.effects.activity = compose.activity
        }
        try {
            compose.setContent { Shots.Looked { Host(app) } }
            waitFor("the app is ready", 60_000) { app.driver.nav.stage == Stage.Ready }
            front(app, "Today")
            assertTrue("navigation is present before the session ends", compose.onAllNodes(androidx.compose.ui.test.hasText("Orders")).fetchSemanticsNodes().isNotEmpty())

            send(app, Events.select(1))
            front(app, "Sign in")
            val modal = app.driver.nav.modals.single()
            assertTrue("the sign-in cover is locked", modal.locked)
            assertEquals("the route asked for a sheet", com.navree.protocol.Navree.Present.SHEET, modal.present)
            assertTrue("no separate dialog window floats over the activity", Shots.windows().none { !it.first })
            assertTrue("a locked cover has no Close button", compose.onAllNodes(hasContentDescription("Close")).fetchSemanticsNodes().isEmpty())
            assertTrue("the screen underneath is gone from semantics", compose.onAllNodes(androidx.compose.ui.test.hasText("Ludhiana warehouse")).fetchSemanticsNodes().isEmpty())
            assertTrue("the destinations underneath are gone from semantics", compose.onAllNodes(androidx.compose.ui.test.hasText("Orders")).fetchSemanticsNodes().isEmpty())

            repeat(6) {
                Shots.pressTab()
                compose.waitForIdle()
                val focused = compose.onAllNodes(isFocused()).fetchSemanticsNodes()
                assertEquals("each press of Tab lands on a visible stop of the cover", 1, focused.size)
                val cfg = focused.single().config
                val words = listOfNotNull(
                    cfg.getOrNull(SemanticsProperties.ContentDescription)?.joinToString(", "),
                    cfg.getOrNull(SemanticsProperties.Text)?.joinToString(", ") { it.text },
                ).joinToString(" ")
                assertTrue("Tab stays inside the sign-in cover, never on Orders or Today: $words", !words.contains("Orders") && !words.contains("Today") && !words.contains("Inventory"))
            }
        } finally {
            compose.runOnUiThread { app.effects.close() }
        }
    }
}
