package com.navree.client

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.navree.client.core.Driver
import com.navree.client.core.Engine
import com.navree.client.core.Events
import com.navree.client.ui.NavreeApp
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The words a person is shown for a notice of the runtime core. The core is
 * stood in for by one that holds a screen and one waiting message, so the
 * message is drawn where the app draws its messages, over a screen: the notice
 * that a change was saved though the screen could not be brought up to date,
 * and a notice a core newer than this build says, which has no words here and
 * is still said.
 */
@RunWith(AndroidJUnit4::class)
class NoticeWordsTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    @Before
    fun readingListsFollowThePictures() {
        Shots.rule = compose
        Shots.tree = null
    }

    /** A core with one screen in front and one message waiting to be shown. */
    private class Holding(private val message: String) : Engine {
        private val nav = """{"stage":"ready","destinations":[{"label":"Orders","href":"/orders","items":[],"stack":[1]}],"selected":0,"modals":[],"front":1,"can_go_back":false,"window":{"width":"compact"}}"""
        private val screen = """{"id":1,"url":"/orders","content":{"t":"native","screen":{"title":"Orders","body":{"t":"text","value":"Order 1042 for Harpreet Sandhu, Ludhiana"}}},
            "loading":false,"refreshing":false,"cached":false,"status":200,"chrome":true,"overlays":[],"states":{},"forms":{},"regions":{},"lists":{}}"""

        override fun send(event: ByteArray): ByteArray {
            val start = Json.parseToJsonElement(event.decodeToString()).jsonObject["t"]?.jsonPrimitive?.content == "start"
            return (if (start) """{"changed":[{"t":"nav"},{"t":"screen","id":1},{"t":"messages"}],"effects":[]}""" else """{"changed":[],"effects":[]}""").encodeToByteArray()
        }

        override fun view(query: ByteArray): ByteArray {
            val part = query.decodeToString().takeIf { it.isNotEmpty() }?.let { Json.parseToJsonElement(it).jsonObject["part"]?.jsonPrimitive?.content }
            return when (part) {
                "nav" -> nav
                "screen" -> screen
                "messages" -> "[$message]"
                else -> "null"
            }.encodeToByteArray()
        }
    }

    private fun words(id: Int): String = InstrumentationRegistry.getInstrumentation().targetContext.getString(id)

    /** Starts the app over a core that holds [message], and waits until [expected] is drawn. */
    private fun shows(message: String, expected: String): Boolean {
        val driver = Driver(Holding(message))
        val config = NavreeConfig(origin = "https://shop.example", name = "Charkha Supply Co.", seed = null)
        compose.setContent { Shots.Looked { NavreeApp(driver, config) } }
        compose.runOnUiThread { driver.send(Events.start) }
        return try {
            compose.waitUntil(10_000) { compose.onAllNodes(hasText(expected, substring = true)).fetchSemanticsNodes().isNotEmpty() }
            true
        } catch (_: Throwable) {
            false
        }
    }

    @Test
    fun aChangeThatWasSavedThoughTheScreenCouldNotBeBroughtUpToDateIsSaidAsSaved() {
        val saved = words(R.string.navree_notice_saved_not_refreshed)
        assertTrue("the words begin by saying it was saved: $saved", saved.startsWith("Saved"))
        assertTrue("the notice is shown in its words: $saved", shows("""{"id":1,"tone":"warning","notice":"saved_not_refreshed"}""", saved))
        Shots.take("notice-saved-not-refreshed")
    }

    @Test
    fun aNoticeThisBuildHasNoWordsForIsStillSaid() {
        assertTrue(
            "a notice of a newer core that says something failed is said as a failure",
            shows("""{"id":1,"tone":"danger","notice":"said_by_a_newer_core"}""", words(R.string.navree_notice_unknown_failed)),
        )
    }

    @Test
    fun aNoticeThisBuildHasNoWordsForThatIsNoFailureIsSaidAsSomethingItCannotDescribe() {
        assertTrue(
            "a notice of a newer core that is no failure is said too, and not as a failure",
            shows("""{"id":1,"tone":"warning","notice":"said_by_a_newer_core"}""", words(R.string.navree_notice_unknown)),
        )
    }
}
