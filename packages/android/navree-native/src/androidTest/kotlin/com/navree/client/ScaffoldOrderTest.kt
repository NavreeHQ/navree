package com.navree.client

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The order a keyboard's Tab goes through the parts that Material's `Scaffold`
 * lays out, in the version the client builds with, in the two shapes the
 * client uses: a bar along the bottom around a screen that has a top bar, and
 * a rail at the start beside a screen that has a top bar. The one-dimensional
 * search goes by the order the layout nodes are placed in, so what the
 * `Scaffold` places first is what Tab reaches first.
 */
@RunWith(AndroidJUnit4::class)
class ScaffoldOrderTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun walk(count: Int): List<String> {
        val stops = ArrayList<String>()
        repeat(count) {
            Shots.pressTab()
            compose.waitForIdle()
            stops += compose.onAllNodes(isFocused()).fetchSemanticsNodes().joinToString("+") { node ->
                node.config.getOrNull(SemanticsProperties.Text)?.joinToString(",") { it.text }.orEmpty()
            }
        }
        return stops
    }

    @Test
    fun aBarAlongTheBottomAroundAScreenWithATopBarIsGoneThroughTopContentBottom() {
        compose.setContent {
            Scaffold(
                bottomBar = { Button(onClick = {}) { Text("Bottom") } },
            ) { padding ->
                Box(Modifier.fillMaxSize().padding(padding)) {
                    Scaffold(topBar = { Button(onClick = {}) { Text("Top") } }) { inner ->
                        Box(Modifier.padding(inner)) { Button(onClick = {}) { Text("Content") } }
                    }
                }
            }
        }
        compose.waitForIdle()
        assertEquals("the stops of a phone's layout, in the order Tab goes", listOf("Top", "Content", "Bottom"), walk(3))
    }

    @Test
    fun aRailAtTheStartBesideAScreenWithATopBarIsGoneThroughRailTopContent() {
        compose.setContent {
            Row(Modifier.fillMaxSize()) {
                Box(Modifier.fillMaxHeight()) { Button(onClick = {}) { Text("Rail") } }
                Box(Modifier.fillMaxSize()) {
                    Scaffold(topBar = { Button(onClick = {}) { Text("Top") } }) { inner ->
                        Box(Modifier.padding(inner)) { Button(onClick = {}) { Text("Content") } }
                    }
                }
            }
        }
        compose.waitForIdle()
        assertEquals("the stops of a tablet's layout, in the order Tab goes", listOf("Rail", "Top", "Content"), walk(3))
    }
}
