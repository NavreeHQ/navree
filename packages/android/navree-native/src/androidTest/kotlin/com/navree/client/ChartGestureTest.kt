package com.navree.client

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.navree.client.ui.ChartView
import com.navree.protocol.Navree
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * What a finger does on a chart: held on it, it reads the mark out; slid
 * along it, it reads the marks out and nothing else moves; moved up or down
 * to scroll the page, it reads nothing and the page moves; and a new answer
 * that arrives under a finger that is down is the one the finger reads.
 */
@RunWith(AndroidJUnit4::class)
class ChartGestureTest {
    @get:Rule
    val compose = createComposeRule()

    private fun chart(prefix: String) = Navree.Chart(
        kind = Navree.ChartKind.BAR,
        labels = listOf("Mon", "Tue", "Wed"),
        series = listOf(Navree.Series(label = "Hours", shown = listOf("${prefix}1", "${prefix}2", "${prefix}3"), values = listOf(1.0, 2.0, 3.0))),
        summary = "Hours by day",
    )

    private val chartNode get() = compose.onNodeWithContentDescription("Hours by day")

    private fun readOut(): String? =
        chartNode.fetchSemanticsNode().config.getOrNull(SemanticsProperties.StateDescription)

    private fun hold() = compose.mainClock.advanceTimeBy(600)

    @Test
    fun a_finger_held_on_a_chart_reads_out_the_mark_under_it_until_it_goes_up() {
        compose.setContent { MaterialTheme { ChartView(chart("a")) } }
        assertEquals("nothing is read before a finger is down", null, readOut())
        chartNode.performTouchInput { down(Offset(width * 0.2f, height * 0.5f)) }
        hold()
        compose.waitForIdle()
        assertEquals("Mon, a1", readOut())
        chartNode.performTouchInput { up() }
        compose.waitForIdle()
        assertEquals(null, readOut())
    }

    @Test
    fun a_finger_slid_along_a_chart_reads_out_each_mark_it_passes() {
        compose.setContent { MaterialTheme { ChartView(chart("a")) } }
        chartNode.performTouchInput {
            down(Offset(width * 0.2f, height * 0.5f))
            moveTo(Offset(width * 0.5f, height * 0.5f))
        }
        compose.waitForIdle()
        assertEquals("Tue, a2", readOut())
        chartNode.performTouchInput { moveTo(Offset(width * 0.9f, height * 0.5f)) }
        compose.waitForIdle()
        assertEquals("Wed, a3", readOut())
        chartNode.performTouchInput { up() }
    }

    @Test
    fun a_new_answer_of_the_same_length_under_a_finger_that_is_down_is_the_one_it_reads() {
        var node by mutableStateOf(chart("a"))
        compose.setContent { MaterialTheme { ChartView(node) } }
        chartNode.performTouchInput { down(Offset(width * 0.2f, height * 0.5f)) }
        hold()
        compose.waitForIdle()
        assertEquals("Mon, a1", readOut())
        node = chart("b")
        compose.waitForIdle()
        chartNode.performTouchInput { moveTo(Offset(width * 0.9f, height * 0.5f)) }
        compose.waitForIdle()
        assertEquals("the finger follows the new answer", "Wed, b3", readOut())
        chartNode.performTouchInput { up() }
    }

    @Test
    fun a_finger_that_scrolls_the_page_from_a_chart_reads_nothing_and_the_page_moves() {
        lateinit var scroll: androidx.compose.foundation.ScrollState
        compose.setContent {
            MaterialTheme {
                scroll = rememberScrollState()
                Column(Modifier.fillMaxWidth().verticalScroll(scroll)) {
                    ChartView(chart("a"))
                    Spacer(Modifier.height(1600.dp))
                }
            }
        }
        chartNode.performTouchInput {
            down(Offset(width * 0.5f, height * 0.8f))
            for (step in 1..8) moveBy(Offset(0f, -30f), delayMillis = 16)
        }
        hold()
        compose.waitForIdle()
        assertTrue("the page scrolled", scroll.value > 0)
        assertEquals("the chart read nothing while the page moved", null, readOut())
        chartNode.performTouchInput { up() }
    }
}
