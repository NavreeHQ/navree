package com.navree.client

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTouchInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.navree.client.core.Content
import com.navree.client.core.Events
import com.navree.client.core.ScreenView
import com.navree.client.core.Stage
import com.navree.client.design.ListRules
import com.navree.client.design.ScreenRules
import com.navree.client.design.walk
import com.navree.client.ui.NavreeApp
import com.navree.protocol.Navree
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The client against the demos themselves, running on the machine the
 * emulator runs on (`-e store http://10.0.2.2:8095`, `-e inbox …`, `-e board …`).
 * Every screen is the server's answer of now, drawn by the client, and
 * photographed: each destination, then what a person does next on it. Without
 * an app's argument its test is skipped.
 */
@RunWith(AndroidJUnit4::class)
class LiveDemosTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val arguments = InstrumentationRegistry.getArguments()

    private fun waitFor(what: String, timeout: Long = 30_000, check: () -> Boolean) {
        try {
            compose.waitUntil(timeout) { check() }
        } catch (e: Throwable) {
            throw AssertionError("waited for $what", e)
        }
        compose.waitForIdle()
    }

    private fun front(app: NavreeSession): ScreenView? = app.driver.nav.front?.let { app.driver.screens[it] }

    /** Waits for the screen in front to show something, and for drawing to settle. */
    private fun settled(app: NavreeSession, title: String? = null) {
        waitFor("a screen${title?.let { " \"$it\"" } ?: ""}") {
            val front = front(app) ?: return@waitFor false
            val tree = (front.content as? Content.Native)?.screen
            !front.loading && (if (title == null) tree != null || front.content is Content.Web else tree?.title == title)
        }
        Thread.sleep(1500)
        compose.waitForIdle()
    }

    private fun send(app: NavreeSession, event: kotlinx.serialization.json.JsonObject) {
        compose.runOnUiThread { app.driver.send(event) }
    }

    private fun start(origin: String): NavreeSession {
        val config = NavreeConfig(origin = origin, name = "Demo", seed = null, loopbackHttp = true, openAnyLink = true)
        lateinit var app: NavreeSession
        compose.runOnUiThread {
            app = NavreeSession(compose.activity.application, config, origin.toHttpUrl())
            app.effects.activity = compose.activity
        }
        compose.setContent { NavreeApp(app.driver, config, image = app::image) }
        waitFor("the app is ready", 60_000) { app.driver.nav.stage == Stage.Ready }
        settled(app)
        return app
    }

    /** Every destination of the app, photographed in turn; ends on the first. */
    private fun destinations(app: NavreeSession, name: String) {
        val count = app.driver.nav.destinations.size
        for (index in 0 until count) {
            send(app, Events.select(index))
            settled(app)
            val label = app.driver.nav.destinations[index].label.lowercase().replace(Regex("[^a-z0-9]+"), "-")
            Shots.take("$name-$label")
        }
        send(app, Events.select(0))
        settled(app)
    }

    /** The first row of the screen's main table or list, by the words it is drawn with. */
    private fun firstRowWords(screen: Navree.Screen): String? {
        ScreenRules.mainTable(screen.body)?.let { table ->
            return table.rows.firstOrNull()?.let { ListRules.asListRow(table.columns, it).headline }
        }
        return screen.body.walk().filterIsInstance<Navree.Row>().firstOrNull { it.action != null }?.title
    }

    /** Opens the first row of the screen in front and photographs what it opened. */
    private fun openFirstRow(app: NavreeSession, shot: String) {
        val screen = front(app)?.screen ?: return
        val words = firstRowWords(screen) ?: return
        val before = front(app)?.id
        compose.onAllNodesWithText(words).onFirst().performClick()
        // What a row opens is the server's to say: a screen, a cover, or a sheet over the list.
        waitFor("what the row opened") {
            val now = front(app)
            now?.id != before || app.driver.nav.modals.isNotEmpty() || now?.overlays?.isNotEmpty() == true
        }
        settled(app)
        Shots.take(shot)
    }

    /** Opens the search view of the screen in front and types, then photographs it. */
    private fun search(app: NavreeSession, words: String, shot: String) {
        val field = compose.onAllNodes(hasSetTextAction()).onFirst()
        runCatching {
            field.performClick()
            field.performTextInput(words)
        }.onFailure { return }
        Thread.sleep(2500)
        compose.waitForIdle()
        Shots.take(shot)
    }

    @Test
    fun theStoreDemo() {
        val origin = arguments.getString("store")
        assumeTrue("no store demo was named", origin != null)
        val app = start(origin!!)
        destinations(app, "store")

        send(app, Events.select(1))
        settled(app)
        // A row held down starts choosing rows, where the table lets them be chosen.
        val screen = front(app)?.screen
        val table = screen?.let { ScreenRules.mainTable(it.body) }
        val words = screen?.let(::firstRowWords)
        if (table?.select != null && words != null) {
            compose.onAllNodesWithText(words).onFirst().performTouchInput { longClick() }
            waitFor("a row chosen", 15_000) { ScreenRules.mainTable(front(app)?.screen?.body)?.rows?.any { it.selected } == true }
            Thread.sleep(800)
            Shots.take("store-orders-selecting")
            send(app, Events.choose(front(app)!!.id, table.id ?: "", null, false))
            settled(app)
        }
        openFirstRow(app, "store-order")
        send(app, Events.back)
        settled(app)
        search(app, "Kaur", "store-orders-search")
    }

    @Test
    fun theInboxDemo() {
        val origin = arguments.getString("inbox")
        assumeTrue("no inbox demo was named", origin != null)
        val app = start(origin!!)
        destinations(app, "inbox")
        openFirstRow(app, "inbox-conversation")
    }

    @Test
    fun theBoardDemo() {
        val origin = arguments.getString("board")
        assumeTrue("no board demo was named", origin != null)
        val app = start(origin!!)
        destinations(app, "board")
        openFirstRow(app, "board-card")
    }
}
