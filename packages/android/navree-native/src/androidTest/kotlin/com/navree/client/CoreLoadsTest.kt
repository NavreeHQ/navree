package com.navree.client

import androidx.test.ext.junit.runners.AndroidJUnit4
import com.navree.core.Core
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/** The runtime core's library loads on a device and answers through its four native methods. */
@RunWith(AndroidJUnit4::class)
class CoreLoadsTest {
    private val config = """
        {"origin":"https://shop.example",
         "client":{"platform":"android","os":"35","form":"phone","app":"1.0.0","lib":"0.1.0","vocab":0,"text_percent":100}}
    """.trimIndent().toByteArray()

    @Test
    fun startingAsksForTheShell() {
        Core(config).use { core ->
            val update = String(core.send("""{"t":"start"}""".toByteArray()))
            assertTrue(update, update.contains("\"t\":\"fetch\""))
            assertTrue(update, update.contains("https://shop.example/_navree/native/shell"))
            val nav = String(core.view("""{"part":"nav"}""".toByteArray()))
            assertTrue(nav, nav.contains("\"stage\":\"starting\""))
        }
    }

    @Test
    fun anEventThatDoesNotReadIsAnError() {
        Core(config).use { core ->
            val answer = String(core.send("not json".toByteArray()))
            assertTrue(answer, answer.contains("\"error\""))
        }
    }
}
