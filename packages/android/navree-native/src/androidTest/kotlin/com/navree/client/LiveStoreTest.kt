package com.navree.client

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.navree.client.core.Content
import com.navree.client.core.Events
import com.navree.client.core.Stage
import com.navree.client.ui.NavreeApp
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The client against the store demo itself, running on the machine the
 * emulator runs on (`-e store http://10.0.2.2:8095`). Every screen is the
 * server's answer of now, drawn by the client, and photographed. Without the
 * argument the test is skipped.
 */
@RunWith(AndroidJUnit4::class)
class LiveStoreTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val origin: String? = InstrumentationRegistry.getArguments().getString("store")

    private fun waitFor(what: String, timeout: Long = 30_000, check: () -> Boolean) {
        try {
            compose.waitUntil(timeout) { check() }
        } catch (e: Throwable) {
            throw AssertionError("waited for $what", e)
        }
        compose.waitForIdle()
    }

    private fun front(session: NavreeSession, title: String? = null) {
        waitFor("the screen \"$title\"") {
            val front = session.driver.nav.front?.let { session.driver.screens[it] } ?: return@waitFor false
            val tree = (front.content as? Content.Native)?.screen
            !front.loading && (if (title == null) tree != null || front.content is Content.Web else tree?.title == title)
        }
        Thread.sleep(1200)
        compose.waitForIdle()
    }

    @Test
    fun theStoreDemo() {
        val store = origin
        assumeTrue("no store demo was named", store != null)
        val config = NavreeConfig(origin = store!!, name = "Charkha Supply Co.", seed = null, loopbackHttp = true, openAnyLink = true)
        lateinit var app: NavreeSession
        compose.runOnUiThread {
            app = NavreeSession(compose.activity.application, config, store.toHttpUrl())
            app.effects.activity = compose.activity
        }
        compose.setContent { NavreeApp(app.driver, config, image = app::image) }
        waitFor("the app is ready") { app.driver.nav.stage == Stage.Ready }

        front(app)
        Shots.take("live-dashboard")

        compose.runOnUiThread { app.driver.send(Events.select(1)) }
        front(app, "Orders")
        Shots.take("live-orders")

        // A row opens what the server says it opens.
        val firstOrder = app.driver.nav.front?.let { app.driver.screens[it] }?.screen
        val rowTitle = firstOrder?.let { com.navree.client.design.ScreenRules.mainTable(it.body) }?.rows?.firstOrNull()
            ?.let { row -> com.navree.client.design.ListRules.asListRow(com.navree.client.design.ScreenRules.mainTable(firstOrder.body)!!.columns, row).headline }
        if (rowTitle != null) {
            compose.onAllNodesWithText(rowTitle).onFirst().performClick()
            // What a row opens is the server's to say: a screen, a cover, or a sheet over the list.
            waitFor("the order opened") {
                val front = app.driver.nav.front?.let { app.driver.screens[it] }
                front?.screen?.title != "Orders" || app.driver.nav.modals.isNotEmpty() || front.overlays.isNotEmpty()
            }
            front(app)
            Shots.take("live-order")
            compose.runOnUiThread { app.driver.send(Events.back) }
            front(app, "Orders")
        }

        compose.runOnUiThread { app.driver.send(Events.select(2)) }
        front(app, "Inventory")
        Shots.take("live-inventory")
    }
}
