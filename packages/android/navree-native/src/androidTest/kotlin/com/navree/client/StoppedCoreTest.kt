package com.navree.client

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.navree.client.core.Driver
import com.navree.client.core.Engine
import com.navree.client.core.Events
import com.navree.client.ui.NavreeApp
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.atomic.AtomicInteger

/**
 * An app whose runtime core has stopped. The core is stood in for by one that
 * answers as the core's C interface does once it has stopped (the words, and
 * the `stopped` mark): the app shows nothing the core held, says it must be
 * started again, and its button calls the restart the host gave it.
 */
@RunWith(AndroidJUnit4::class)
class StoppedCoreTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    @Before
    fun readingListsFollowThePictures() {
        Shots.rule = compose
        Shots.tree = null
    }

    /** A core that is going, then has stopped: every call answers that it has, as the real one does. */
    private class Stopping : Engine {
        @Volatile
        var stopped = false
        val sent = AtomicInteger()
        private val gone = """{"error":"the core stopped after a failure inside it and can't be used again","stopped":true}"""

        override fun send(event: ByteArray): ByteArray {
            sent.incrementAndGet()
            return (if (stopped) gone else """{"changed":[],"effects":[]}""").encodeToByteArray()
        }

        override fun view(query: ByteArray): ByteArray = (if (stopped) gone else "null").encodeToByteArray()
    }

    private fun words(id: Int): String = InstrumentationRegistry.getInstrumentation().targetContext.getString(id)

    @Test
    fun anAppWhoseCoreHasStoppedSaysItMustBeStartedAgainAndOffersTo() {
        val engine = Stopping()
        val driver = Driver(engine)
        val again = AtomicInteger()
        val config = NavreeConfig(origin = "https://shop.example", name = "Charkha Supply Co.", seed = null)
        compose.setContent { Shots.Looked { NavreeApp(driver, config, startAgain = { again.incrementAndGet() }) } }
        compose.runOnUiThread { driver.send(Events.start) }
        compose.waitForIdle()
        assertTrue("a core that answers is not a stopped one", !driver.stopped)
        assertEquals("nothing offers a restart while the core answers", 0, compose.onAllNodes(hasText(words(R.string.navree_stopped_action))).fetchSemanticsNodes().size)

        engine.stopped = true
        compose.runOnUiThread { driver.send(Events.back) }
        compose.waitUntil(10_000) { compose.onAllNodes(hasText(words(R.string.navree_stopped_title))).fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText(words(R.string.navree_stopped_title)).assertIsDisplayed()
        compose.onNodeWithText(words(R.string.navree_stopped_text)).assertIsDisplayed()
        Shots.take("core-stopped")
        val asked = engine.sent.get()
        compose.runOnUiThread { driver.send(Events.foreground) }
        assertEquals("a core that stopped is asked nothing more", asked, engine.sent.get())

        compose.onNode(hasText(words(R.string.navree_stopped_action)) and hasClickAction()).performClick()
        compose.waitForIdle()
        assertEquals("the button is the restart the host gave", 1, again.get())
    }
}
