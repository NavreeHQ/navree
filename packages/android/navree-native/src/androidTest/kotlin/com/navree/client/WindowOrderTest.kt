package com.navree.client

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.navree.client.ui.BottomNavigationWindow
import com.navree.client.ui.KeyboardEntry
import com.navree.client.ui.ScreenFrame
import com.navree.client.ui.SideNavigationWindow
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The order a keyboard's Tab goes through the parts of the client's own two
 * window layouts (the real composables, with stand-in parts of two controls
 * each): on a phone the top bar, the content, the bottom navigation; on a
 * tablet the navigation at the start, the top bar, the content. Shift+Tab is
 * the exact mirror, entering each part at its last control. The one-dimensional
 * focus search goes by the order the layout nodes are placed in, so the order
 * of the parts is the order the layouts place them. The windows are drawn in
 * the run's look, so the right to left look walks them laid out right to left:
 * the order is the same in both directions, the navigation at the start edge
 * first on a tablet wherever that edge is. With the focus nowhere the first
 * Tab comes in at the first control of that order (the window's default
 * focus, [KeyboardEntry]), and not at the control nearest the top left corner.
 */
@RunWith(AndroidJUnit4::class)
class WindowOrderTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    /** A part of a window: two controls that say which part they are in. */
    @Composable
    private fun Part(name: String, modifier: Modifier = Modifier) {
        Row(modifier) {
            Button(onClick = {}) { Text("$name 1") }
            Button(onClick = {}) { Text("$name 2") }
        }
    }

    private fun stop(): String =
        compose.onAllNodes(isFocused()).fetchSemanticsNodes().joinToString("+") { node ->
            node.config.getOrNull(SemanticsProperties.Text)?.joinToString(",") { it.text }.orEmpty()
        }

    private fun forward(count: Int): List<String> {
        val stops = ArrayList<String>()
        repeat(count) {
            Shots.pressTab()
            compose.waitForIdle()
            stops += stop()
        }
        return stops
    }

    private fun backward(count: Int): List<String> {
        val stops = ArrayList<String>()
        repeat(count) {
            Shots.pressShiftTab()
            compose.waitForIdle()
            stops += stop()
        }
        return stops
    }

    /**
     * Tab through every control, then Shift+Tab back: each press says where it
     * landed. The order Tab goes round in and the control it came in at are
     * judged apart, the first being the layout's and the second the window's
     * default focus: a failure says which of the two it is.
     */
    private fun assertOrder(what: String, expected: List<String>) {
        val there = forward(expected.size)
        val from = expected.indexOf(there.first())
        assertTrue("the first stop of $what is one of its controls (look ${Shots.look}): $there", from >= 0)
        assertEquals("the order Tab goes round $what in, from the control it came in at (look ${Shots.look})", expected.drop(from) + expected.take(from), there)
        assertEquals("the control a keyboard comes into $what at, with the focus nowhere (look ${Shots.look}): $there", expected.first(), there.first())
        val back = backward(expected.size - 1)
        assertEquals("the stops of $what, in the order Shift+Tab goes back from the last", expected.dropLast(1).reversed(), back)
    }

    @Test
    fun aPhonesWindowIsGoneThroughTopContentBottomAndBackTheOtherWay() {
        compose.setContent {
            Shots.Looked {
                KeyboardEntry()
                BottomNavigationWindow(bottomBar = { Part("Bottom") }) {
                    ScreenFrame(topBarShown = true, topBar = { Part("Top") }, snackbarHost = {}) { padding ->
                        Box(Modifier.padding(padding)) { Part("Content") }
                    }
                }
            }
        }
        compose.waitForIdle()
        assertOrder("a phone's window", listOf("Top 1", "Top 2", "Content 1", "Content 2", "Bottom 1", "Bottom 2"))
    }

    @Test
    fun aTabletsWindowIsGoneThroughNavigationTopContentAndBackTheOtherWay() {
        compose.setContent {
            Shots.Looked {
                KeyboardEntry()
                SideNavigationWindow(rail = { Column { Button(onClick = {}) { Text("Rail 1") }; Button(onClick = {}) { Text("Rail 2") } } }) {
                    ScreenFrame(topBarShown = true, topBar = { Part("Top") }, snackbarHost = {}) { padding ->
                        Box(Modifier.padding(padding)) { Part("Content") }
                    }
                }
            }
        }
        compose.waitForIdle()
        assertOrder("a tablet's window", listOf("Rail 1", "Rail 2", "Top 1", "Top 2", "Content 1", "Content 2"))
    }
}
