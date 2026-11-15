package com.navree.client

import androidx.activity.ComponentActivity
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeUp
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.navree.client.core.Content
import com.navree.client.core.Stage
import mockwebserver3.MockWebServer
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.After
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * A board whose first lane holds 20, 200 and 2,000 cards (the board demo's
 * server sends 20 at first and 20 more at a time, up to 200 in a lane): the
 * time to the first screen drawn, the time of ten swipes up the page and how
 * many of the lane's cards are drawn at the end, written beside the pictures
 * as `lanes-<device>-<look>-<cards>.txt`. A lane is laid lazily, so whatever it
 * holds the first screen of 2,000 cards comes in the time of one screen, and
 * only the cards near the window are drawn.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(AndroidJUnit4::class)
class LaneScaleTest {
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

    private fun measure(cards: Int) {
        val body = LaneBoard.json(cards)
        server.dispatcher = Recorded("", routes = mapOf("/_navree/native/shell" to "shell", "/" to "screen-board"), read = { file -> if (file == "screen-board.json") body else null })
        server.start()
        val origin = server.url("/").toString().trimEnd('/')
        val config = NavreeConfig(origin = origin, name = "Board", seed = null, loopbackHttp = true, openAnyLink = true)
        lateinit var made: NavreeSession
        compose.runOnUiThread {
            made = NavreeSession(compose.activity.application, config, origin.toHttpUrl())
            made.effects.activity = compose.activity
        }
        val began = System.nanoTime()
        compose.setContent { Shots.Looked { Host(made) } }
        compose.waitUntil(180_000) {
            val front = made.driver.nav.front?.let { made.driver.screens[it] }
            (front?.content as? Content.Native)?.screen?.title == "Board" && !front.loading
        }
        compose.waitForIdle()
        val firstMs = (System.nanoTime() - began) / 1_000_000
        val drawnFirst = cardsDrawn()
        Shots.take("lane-$cards-first")
        val before = cardNumbers()
        val stats = FrameStats(compose.activity.window)
        compose.runOnUiThread { stats.start() }
        val scrolled = System.nanoTime()
        repeat(10) {
            compose.onRoot().performTouchInput { swipeUp(startY = bottom * 0.85f, endY = bottom * 0.2f, durationMillis = 200) }
            compose.waitForIdle()
        }
        val scrollMs = (System.nanoTime() - scrolled) / 1_000_000
        val frames = compose.runOnUiThread { stats.stop() }
        val after = cardNumbers()
        val drawn = cardsDrawn()
        val heap = Runtime.getRuntime().let { (it.totalMemory() - it.freeMemory()) / (1024 * 1024) }
        Shots.take("lane-$cards-scrolled")
        Shots.put(
            "lanes-${Shots.device}-${Shots.look}-$cards.txt",
            "cards in the first lane: $cards\nfirst screen drawn: $firstMs ms\nten swipes up the page: $scrollMs ms\n" +
                "cards of the lane drawn at the first screen: $drawnFirst, after the swipes: $drawn\nheap in use afterwards: $heap MB\n" +
                "the ten swipes moved the cards drawn from ${before.minOrNull()}..${before.maxOrNull()} to ${after.minOrNull()}..${after.maxOrNull()}\n" +
                "frames of the ten swipes: $frames\n",
        )
        assertTrue("the board with $cards cards in a lane came up with cards in it", drawnFirst > 0)
        assertTrue("$drawnFirst cards of the lane are drawn at the first screen of $cards: only the ones near the window are", drawnFirst < MOST_DRAWN)
        assertTrue("$drawn cards of the lane are drawn after ten swipes: only the ones near the window are", drawn < MOST_DRAWN)
        if (cards >= 2000) {
            assertTrue("the first screen of a lane of $cards cards took $firstMs ms, over $MOST_FIRST_MS", firstMs < MOST_FIRST_MS)
        }
    }

    /** The numbers of the first lane's cards that are drawn now. */
    private fun cardNumbers(): List<Int> =
        compose.onAllNodes(hasText(LaneBoard.TITLE_END, substring = true)).fetchSemanticsNodes().mapNotNull { node ->
            val words = node.config.getOrNull(SemanticsProperties.Text)?.joinToString(" ") { it.text }.orEmpty()
            Regex("Card (\\d+) of").find(words)?.groupValues?.get(1)?.toInt()
        }

    /** How many of the first lane's cards are drawn now: each is a node that says its title. */
    private fun cardsDrawn(): Int =
        compose.onAllNodes(hasText(LaneBoard.TITLE_END, substring = true)).fetchSemanticsNodes().size

    @Test
    fun aLaneOfTwentyCards() = measure(20)

    @Test
    fun aLaneOfTwoHundredCards() = measure(200)

    @Test
    fun aLaneOfTwoThousandCards() = measure(2000)
}

private const val MOST_DRAWN = 60
private const val MOST_FIRST_MS = 1_500L
