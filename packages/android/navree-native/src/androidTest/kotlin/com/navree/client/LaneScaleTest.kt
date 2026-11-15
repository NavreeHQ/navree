package com.navree.client

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeUp
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.navree.client.core.Content
import com.navree.client.core.Stage
import mockwebserver3.MockWebServer
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * A board whose first lane holds 20, 200 and 2,000 cards (the board demo's
 * server sends 20 at first and 20 more at a time, up to 200 in a lane): the
 * time to the first screen drawn and the time of ten swipes up the page, written
 * beside the pictures as `lanes-<device>-<look>-<cards>.txt`.
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

    /** The recorded board with `cards` cards in its first lane. */
    private fun boardWith(cards: Int): String {
        val text = InstrumentationRegistryAssets.read("screen-board.json")
        val root = JSONObject(text)
        fun visit(node: Any?) {
            when (node) {
                is JSONObject -> {
                    if (node.optString("id") == "board-todo" && node.has("reorder")) {
                        val children = node.getJSONArray("children")
                        val first = children.getJSONObject(0)
                        val more = JSONArray()
                        for (at in 0 until cards) {
                            val copy = JSONObject(first.toString())
                            copy.put("id", "c-$at")
                            copy.put("key", "$at")
                            copy.put("title", "Card $at of the first lane")
                            copy.getJSONObject("action").put("href", "/cards/c-$at")
                            more.put(copy)
                        }
                        node.put("children", more)
                    }
                    node.keys().forEach { visit(node.get(it)) }
                }
                is JSONArray -> for (at in 0 until node.length()) visit(node.get(at))
            }
        }
        visit(root)
        return root.toString()
    }

    private fun measure(cards: Int) {
        val body = boardWith(cards)
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
        val scrolled = System.nanoTime()
        repeat(10) {
            compose.onRoot().performTouchInput { swipeUp(startY = bottom * 0.85f, endY = bottom * 0.2f, durationMillis = 200) }
            compose.waitForIdle()
        }
        val scrollMs = (System.nanoTime() - scrolled) / 1_000_000
        val heap = Runtime.getRuntime().let { (it.totalMemory() - it.freeMemory()) / (1024 * 1024) }
        Shots.put("lanes-${Shots.device}-${Shots.look}-$cards.txt", "cards in the first lane: $cards\nfirst screen drawn: $firstMs ms\nten swipes up the page: $scrollMs ms\nheap in use afterwards: $heap MB\n")
        assertTrue("the board with $cards cards in a lane came up and scrolled", firstMs > 0)
    }

    @Test
    fun aLaneOfTwentyCards() = measure(20)

    @Test
    fun aLaneOfTwoHundredCards() = measure(200)

    @Test
    fun aLaneOfTwoThousandCards() = measure(2000)
}

/** The test's own assets, by file name. */
private object InstrumentationRegistryAssets {
    fun read(file: String): String =
        androidx.test.platform.app.InstrumentationRegistry.getInstrumentation().context.assets.open(file).use { it.readBytes().decodeToString() }
}
