package com.navree.client

import androidx.activity.ComponentActivity
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.text.TextLayoutResult
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.navree.client.core.Content
import com.navree.client.core.Stage
import mockwebserver3.MockWebServer
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The labels of the navigation, in scripts the 6.5 dp a glyph estimate cannot
 * tell (Devanagari, Gurmukhi, Chinese with no space to break at), laid out at
 * the text size of the look: none is broken in the middle of its word, so each
 * stands on one line, whatever the look's text size.
 */
@RunWith(AndroidJUnit4::class)
class NavigationLabelTest {
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

    @Test
    fun aLabelInAScriptWithWideGlyphsOrNoSpacesIsNotBrokenInTheMiddleOfItsWord() {
        val labels = listOf("डैशबोर्ड", "ਇਨਵੈਂਟਰੀ", "库存管理总览")
        val shell = InstrumentationRegistry.getInstrumentation().context.assets.open("shell.json").use { it.readBytes().decodeToString() }
            .replace("\"Dashboard\"", "\"${labels[0]}\"").replace("\"Orders\"", "\"${labels[1]}\"").replace("\"Inventory\"", "\"${labels[2]}\"")
        server.dispatcher = Recorded("", routes = mapOf("/_navree/native/shell" to "shell", "/" to "screen-dashboard"), read = { file -> if (file == "shell.json") shell else null })
        server.start()
        val origin = server.url("/").toString().trimEnd('/')
        val config = NavreeConfig(origin = origin, name = "Charkha Supply Co.", seed = null, loopbackHttp = true, openAnyLink = true)
        lateinit var made: NavreeSession
        compose.runOnUiThread {
            made = NavreeSession(compose.activity.application, config, origin.toHttpUrl())
            made.effects.activity = compose.activity
        }
        compose.setContent { Shots.Looked { Host(made) } }
        compose.waitUntil(60_000) { made.driver.nav.stage == Stage.Ready }
        compose.waitUntil(20_000) {
            val front = made.driver.nav.front?.let { made.driver.screens[it] }
            front?.content is Content.Native && !front.loading
        }
        compose.waitForIdle()
        for (label in labels) {
            val nodes = compose.onAllNodes(hasText(label), useUnmergedTree = true).fetchSemanticsNodes()
            assertTrue("the label $label is in the navigation", nodes.isNotEmpty())
            val lines = nodes.mapNotNull { node ->
                val results = ArrayList<TextLayoutResult>()
                node.config.getOrNull(SemanticsActions.GetTextLayoutResult)?.action?.invoke(results)
                results.firstOrNull()?.lineCount
            }
            assertTrue("the label $label has a text layout to read", lines.isNotEmpty())
            for (count in lines) assertEquals("the label $label of one word is on one line at this text size", 1, count)
        }
    }
}
