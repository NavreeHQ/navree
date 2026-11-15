package com.navree.client

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.navree.client.ui.rememberTallest
import com.navree.client.ui.sameHeightAs
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/** A line of cards as tall as the tallest: its record of the tallest goes when the width the cards are laid in changes. */
@RunWith(AndroidJUnit4::class)
class TallestTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun aLineIsNotLeftAsTallAsItWasWhenItsCardsBecomeWiderAndShorter() {
        var width by mutableFloatStateOf(180f)
        compose.setContent {
            MaterialTheme {
                val tallest = rememberTallest("the same parts", width)
                Box(Modifier.width(width.dp).sameHeightAs(tallest).testTag("card")) {
                    Text("A long line of words that wraps to several lines in a narrow card and to one in a wide card.")
                }
            }
        }
        compose.waitForIdle()
        val narrow = compose.onNodeWithTag("card").fetchSemanticsNode().size.height
        compose.runOnUiThread { width = 900f }
        compose.waitForIdle()
        // The settling takes a frame or two.
        Thread.sleep(300)
        compose.waitForIdle()
        val wide = compose.onNodeWithTag("card").fetchSemanticsNode().size.height
        assertTrue("the card is shorter when it is wide ($wide px) than when it was narrow ($narrow px)", wide < narrow)
    }
}
